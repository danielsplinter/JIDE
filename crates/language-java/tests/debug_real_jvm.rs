//! Validação contra uma JVM real, quando houver JDK na máquina.
//!
//! Compila um programa, sobe a JVM com o agente de depuração e percorre o fluxo
//! completo: conexão, breakpoint instalado só quando a classe é carregada,
//! parada, pilha, variáveis, passo e desconexão.
//!
//! Marcado como `ignored` porque depende de um JDK instalado:
//!
//! ```text
//! cargo test -p java-debug-adapter -- --ignored --nocapture
//! ```

use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    sync::{Arc, Mutex},
    time::Duration,
};

use ide_debug_api::{
    DebugAdapter, DebugEvent, DebugEventSink, DebugSessionRequest, DebugTarget, SourceBreakpoint,
    StepKind, StopReason,
};
use language_java::JavaDebugAdapter;
use tokio::{net::TcpListener, sync::Notify};

const PROGRAM: &str = r#"package com.example;

public class Loop {
    public static void main(String[] args) throws Exception {
        int total = 0;
        for (int index = 0; index < 200; index++) {
            total = total + index;
            Thread.sleep(50);
        }
        System.out.println(total);
    }
}
"#;

/// Linha 7 do arquivo, 0-based: `total = total + index;`
const BREAKPOINT_LINE: u32 = 6;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<DebugEvent>>,
    notify: Notify,
}

impl RecordingSink {
    fn events(&self) -> Vec<DebugEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    async fn wait_for(&self, predicate: impl Fn(&DebugEvent) -> bool) -> Option<DebugEvent> {
        for _ in 0..100 {
            if let Some(event) = self.events().into_iter().find(|event| predicate(event)) {
                return Some(event);
            }
            let _ = tokio::time::timeout(Duration::from_millis(200), self.notify.notified()).await;
        }
        None
    }
}

impl DebugEventSink for RecordingSink {
    fn emit(&self, event: DebugEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
        self.notify.notify_waiters();
    }
}

struct Jvm {
    process: Child,
}

impl Drop for Jvm {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn jdk_executable(name: &str) -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin").join(&executable);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(&executable))
        .find(|candidate| candidate.is_file())
}

async fn free_port() -> u16 {
    match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(5005),
        Err(_) => 5005,
    }
}

#[tokio::test]
#[ignore = "requires a JDK"]
async fn debugs_a_real_jvm_end_to_end() {
    let (Some(javac), Some(java)) = (jdk_executable("javac"), jdk_executable("java")) else {
        eprintln!("nenhum JDK encontrado; teste ignorado");
        return;
    };

    let root = std::env::temp_dir().join(format!("er-ide-real-jvm-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let source_root = root.join("src/main/java");
    let package = source_root.join("com/example");
    assert!(fs::create_dir_all(&package).is_ok());
    let source = package.join("Loop.java");
    assert!(fs::write(&source, PROGRAM).is_ok());
    let classes = root.join("classes");

    let compiled = Command::new(&javac)
        .arg("-g")
        .arg("-d")
        .arg(&classes)
        .arg(&source)
        .output();
    match compiled {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!("javac falhou: {}", String::from_utf8_lossy(&output.stderr)),
        Err(error) => panic!("javac não pôde ser executado: {error}"),
    }

    let port = free_port().await;
    let process = Command::new(&java)
        .arg(format!(
            "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"
        ))
        .arg("-cp")
        .arg(&classes)
        .arg("com.example.Loop")
        .spawn();
    let _jvm = match process {
        Ok(process) => Jvm { process },
        Err(error) => panic!("a JVM não pôde ser iniciada: {error}"),
    };

    let sink = Arc::new(RecordingSink::default());
    let adapter = JavaDebugAdapter::new();
    // A JVM leva um instante para abrir a porta de depuração.
    let mut attached = None;
    for _ in 0..40 {
        let request = DebugSessionRequest::new(DebugTarget::new("127.0.0.1", port))
            .with_source_roots(vec![source_root.clone()]);
        match adapter
            .attach(request, Arc::clone(&sink) as Arc<dyn DebugEventSink>)
            .await
        {
            Ok(session) => {
                attached = Some(session);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    let session = match attached {
        Some(session) => session,
        None => panic!("não foi possível conectar à JVM em 127.0.0.1:{port}"),
    };

    let resolved = match session
        .set_breakpoints(&source, &[SourceBreakpoint::new(&source, BREAKPOINT_LINE)])
        .await
    {
        Ok(resolved) => resolved,
        Err(error) => panic!("set_breakpoints falhou: {error}"),
    };
    assert_eq!(resolved.len(), 1);
    assert!(
        !resolved[0].is_verified(),
        "com suspend=y a classe da aplicação ainda não foi carregada"
    );

    // Libera a JVM suspensa na inicialização.
    assert!(session.resume(None).await.is_ok());

    let stopped = sink
        .wait_for(|event| matches!(event, DebugEvent::Stopped { .. }))
        .await;
    let Some(DebugEvent::Stopped { thread, reason }) = stopped else {
        panic!("a JVM não parou no breakpoint: {:?}", sink.events());
    };
    assert!(matches!(reason, StopReason::Breakpoint(_)));

    let frames = match session.stack_trace(thread).await {
        Ok(frames) => frames,
        Err(error) => panic!("stack_trace falhou: {error}"),
    };
    let top = match frames.first() {
        Some(frame) => frame,
        None => panic!("pilha vazia"),
    };
    assert_eq!(top.name, "Loop.main");
    let location = match top.location.as_ref() {
        Some(location) => location,
        None => panic!("quadro sem fonte"),
    };
    assert_eq!(location.path, source);
    assert_eq!(location.range.start.line, BREAKPOINT_LINE);

    let variables = match session.variables(thread, top.id).await {
        Ok(variables) => variables,
        Err(error) => panic!("variables falhou: {error}"),
    };
    let names: Vec<&str> = variables
        .iter()
        .map(|variable| variable.name.as_str())
        .collect();
    assert!(names.contains(&"total"), "variáveis lidas: {names:?}");
    assert!(names.contains(&"index"), "variáveis lidas: {names:?}");
    assert!(names.contains(&"args"), "variáveis lidas: {names:?}");

    let evaluated = match session.evaluate(thread, top.id, "index").await {
        Ok(evaluated) => evaluated,
        Err(error) => panic!("evaluate falhou: {error}"),
    };
    assert!(
        evaluated.value.parse::<i64>().is_ok(),
        "index deveria ser numérico, veio `{}`",
        evaluated.value
    );

    // Passo sobre: a próxima parada é a linha seguinte do laço.
    assert!(session.step(thread, StepKind::Over).await.is_ok());
    let stepped = sink
        .wait_for(|event| {
            matches!(
                event,
                DebugEvent::Stopped {
                    reason: StopReason::Step,
                    ..
                }
            )
        })
        .await;
    let Some(DebugEvent::Stopped { thread, .. }) = stepped else {
        panic!("o passo não produziu parada: {:?}", sink.events());
    };
    let after_step = match session.stack_trace(thread).await {
        Ok(frames) => frames,
        Err(error) => panic!("stack_trace após o passo falhou: {error}"),
    };
    let line = after_step
        .first()
        .and_then(|frame| frame.location.as_ref())
        .map(|location| location.range.start.line);
    assert_eq!(
        line,
        Some(BREAKPOINT_LINE + 1),
        "o passo deveria avançar para `Thread.sleep`"
    );

    assert!(session.detach().await.is_ok());
    let _ = fs::remove_dir_all(root);
}

const BOXED_PROGRAM: &str = r#"package com.example;

public class Boxed {
    Long id = 1L;
    String nome = null;

    void setId(Long id) { this.id = id; }
    void setNome(String nome) { this.nome = nome; }
    long dobro(long value) { return value * 2; }

    public static void main(String[] args) throws Exception {
        Boxed m = new Boxed();
        for (int index = 0; index < 200; index++) {
            Thread.sleep(50);
        }
        System.out.println(m.id);
    }
}
"#;

/// Linha 14 do arquivo, 0-based: `Thread.sleep(50);`
const BOXED_BREAKPOINT_LINE: u32 = 13;

/// Cenário relatado: `m.setId(4L)` num campo `java.lang.Long`.
///
/// O alvo não confere o tipo do argumento antes de repassá-lo à chamada; enviar
/// o `long` cru onde se espera uma referência fazia a JVM ler o número como
/// endereço e morrer. Este teste vale por isso: ele afirma que a chamada tem
/// efeito **e** que o processo continua de pé depois dela.
#[tokio::test]
#[ignore = "requires a JDK"]
async fn invoking_with_a_boxed_parameter_changes_the_object_and_keeps_the_target_alive() {
    let (Some(javac), Some(java)) = (jdk_executable("javac"), jdk_executable("java")) else {
        eprintln!("nenhum JDK encontrado; teste ignorado");
        return;
    };

    let root = std::env::temp_dir().join(format!("er-ide-boxed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let source_root = root.join("src/main/java");
    let package = source_root.join("com/example");
    assert!(fs::create_dir_all(&package).is_ok());
    let source = package.join("Boxed.java");
    assert!(fs::write(&source, BOXED_PROGRAM).is_ok());
    let classes = root.join("classes");

    match Command::new(&javac)
        .arg("-g")
        .arg("-d")
        .arg(&classes)
        .arg(&source)
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!("javac falhou: {}", String::from_utf8_lossy(&output.stderr)),
        Err(error) => panic!("javac não pôde ser executado: {error}"),
    }

    let port = free_port().await;
    let process = Command::new(&java)
        .arg(format!(
            "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"
        ))
        .arg("-cp")
        .arg(&classes)
        .arg("com.example.Boxed")
        .spawn();
    let _jvm = match process {
        Ok(process) => Jvm { process },
        Err(error) => panic!("a JVM não pôde ser iniciada: {error}"),
    };

    let sink = Arc::new(RecordingSink::default());
    let adapter = JavaDebugAdapter::new();
    let mut attached = None;
    for _ in 0..40 {
        let request = DebugSessionRequest::new(DebugTarget::new("127.0.0.1", port))
            .with_source_roots(vec![source_root.clone()]);
        match adapter
            .attach(request, Arc::clone(&sink) as Arc<dyn DebugEventSink>)
            .await
        {
            Ok(session) => {
                attached = Some(session);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    let Some(session) = attached else {
        panic!("não foi possível conectar à JVM em 127.0.0.1:{port}");
    };

    assert!(
        session
            .set_breakpoints(
                &source,
                &[SourceBreakpoint::new(&source, BOXED_BREAKPOINT_LINE)]
            )
            .await
            .is_ok()
    );
    assert!(session.resume(None).await.is_ok());

    let stopped = sink
        .wait_for(|event| matches!(event, DebugEvent::Stopped { .. }))
        .await;
    let Some(DebugEvent::Stopped { thread, .. }) = stopped else {
        panic!("a JVM não parou no breakpoint: {:?}", sink.events());
    };
    let frames = match session.stack_trace(thread).await {
        Ok(frames) => frames,
        Err(error) => panic!("stack_trace falhou: {error}"),
    };
    let Some(frame) = frames.first().map(|frame| frame.id) else {
        panic!("pilha vazia");
    };

    // A chamada que derrubava o processo.
    if let Err(error) = session.evaluate(thread, frame, "m.setId(4L);").await {
        panic!("setId(Long) falhou: {error}");
    }
    // Se a JVM tivesse morrido, esta leitura voltaria `Detached`.
    let id = match session.evaluate(thread, frame, "m.id.value").await {
        Ok(id) => id,
        Err(error) => panic!("a sessão não sobreviveu à chamada: {error}"),
    };
    assert_eq!(id.value, "4", "o campo deveria ter o valor atribuído");

    // Texto vira uma `String` criada no alvo.
    if let Err(error) = session.evaluate(thread, frame, r#"m.setNome("oi")"#).await {
        panic!("setNome(String) falhou: {error}");
    }
    let nome = match session.evaluate(thread, frame, "m.nome").await {
        Ok(nome) => nome,
        Err(error) => panic!("leitura do nome falhou: {error}"),
    };
    assert_eq!(nome.value, "\"oi\"");

    // Um parâmetro primitivo continua indo direto, e o retorno é lido.
    let dobro = match session.evaluate(thread, frame, "m.dobro(21L)").await {
        Ok(dobro) => dobro,
        Err(error) => panic!("dobro(long) falhou: {error}"),
    };
    assert_eq!(dobro.value, "42");

    // Instruções em sequência se acumulam no mesmo objeto: a segunda encontra o
    // que a primeira deixou.
    for instrucao in ["m.setId(7L)", "m.setNome(\"Mario\")"] {
        if let Err(error) = session.evaluate(thread, frame, instrucao).await {
            panic!("`{instrucao}` falhou: {error}");
        }
    }
    let acumulado = match session.evaluate(thread, frame, "m.id.value").await {
        Ok(valor) => valor,
        Err(error) => panic!("leitura após a sequência falhou: {error}"),
    };
    assert_eq!(acumulado.value, "7");
    assert_eq!(
        session
            .evaluate(thread, frame, "m.nome")
            .await
            .map(|nome| nome.value)
            .unwrap_or_default(),
        "\"Mario\""
    );

    // Argumento que não cabe é recusado aqui, sem chegar ao alvo.
    let recusada = session.evaluate(thread, frame, "m.dobro(true)").await;
    assert!(
        recusada.is_err(),
        "um booleano não deveria ser enviado onde se espera um long"
    );
    assert!(
        session.evaluate(thread, frame, "m.id.value").await.is_ok(),
        "a recusa não pode ter custado a sessão"
    );

    assert!(session.detach().await.is_ok());
    let _ = fs::remove_dir_all(root);
}

/// Cenário relatado: subir um Spring Boot em depuração, marcar breakpoint num
/// controller e chamar o endpoint.
///
/// A ordem é a mesma da IDE: conecta assim que a porta abre — com a aplicação
/// ainda subindo —, registra o breakpoint numa classe que só será carregada
/// depois, e espera a parada quando a requisição chega.
#[tokio::test]
#[ignore = "requires Maven and the example Spring Boot project"]
async fn stops_on_a_controller_breakpoint_of_a_running_spring_boot_application() {
    let project =
        PathBuf::from("C:/Users/jdani/Documents/projetos/java/spring-boot-four-endpoints");
    if !project.join("pom.xml").is_file() {
        eprintln!("projeto de exemplo ausente; teste ignorado");
        return;
    }
    let source_root = project.join("src/main/java");
    let controller = source_root.join("br/com/exemplo/endpoints/controller/ItemController.java");
    assert!(
        controller.is_file(),
        "controller ausente: {}",
        controller.display()
    );

    let debug_port = free_port().await;
    let agent = format!(
        "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=127.0.0.1:{debug_port}"
    );
    let process = Command::new("cmd")
        .arg("/C")
        .arg("mvn")
        .arg("-B")
        .arg("-Dmaven.test.skip=true")
        .arg("spring-boot:run")
        .arg(format!("-Dspring-boot.run.jvmArguments={agent}"))
        .current_dir(&project)
        .spawn();
    let _jvm = match process {
        Ok(process) => Jvm { process },
        Err(error) => panic!("Maven não pôde ser iniciado: {error}"),
    };

    let sink = Arc::new(RecordingSink::default());
    let adapter = JavaDebugAdapter::new();
    let mut attached = None;
    for _ in 0..180 {
        let request = DebugSessionRequest::new(DebugTarget::new("127.0.0.1", debug_port))
            .with_source_roots(vec![source_root.clone()]);
        match adapter
            .attach(request, Arc::clone(&sink) as Arc<dyn DebugEventSink>)
            .await
        {
            Ok(session) => {
                attached = Some(session);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let session = match attached {
        Some(session) => session,
        None => panic!("não foi possível conectar em 127.0.0.1:{debug_port}"),
    };

    // Linha 63 do arquivo, 0-based 62: o `System.out.printf` de `executarLaco`.
    let resolved = match session
        .set_breakpoints(&controller, &[SourceBreakpoint::new(&controller, 62)])
        .await
    {
        Ok(resolved) => resolved,
        Err(error) => panic!("set_breakpoints falhou: {error}"),
    };
    eprintln!("breakpoint: {resolved:?}");

    // Espera a aplicação atender antes de chamar o endpoint.
    let mut serving = false;
    for _ in 0..240 {
        if std::net::TcpStream::connect_timeout(
            &match "127.0.0.1:8080".parse() {
                Ok(address) => address,
                Err(error) => panic!("endereço inválido: {error}"),
            },
            Duration::from_millis(200),
        )
        .is_ok()
        {
            serving = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(serving, "a aplicação precisa atender na porta 8080");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // A requisição roda em outra thread: a parada não pode travar o teste.
    std::thread::spawn(|| {
        use std::io::Write;
        if let Ok(mut stream) = std::net::TcpStream::connect("127.0.0.1:8080") {
            let _ = stream.write_all(
                b"GET /api/itens HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            );
        }
    });

    let stopped = sink
        .wait_for(|event| matches!(event, DebugEvent::Stopped { .. }))
        .await;
    let Some(DebugEvent::Stopped { thread, reason }) = stopped else {
        panic!(
            "a aplicação não parou no breakpoint. eventos: {:?}",
            sink.events()
        );
    };
    assert!(matches!(reason, StopReason::Breakpoint(_)));

    let frames = match session.stack_trace(thread).await {
        Ok(frames) => frames,
        Err(error) => panic!("stack_trace falhou: {error}"),
    };
    let top = match frames.first() {
        Some(frame) => frame,
        None => panic!("pilha vazia"),
    };
    eprintln!("parou em {} {:?}", top.name, top.location);
    assert!(top.name.contains("ItemController"));
    assert_eq!(
        top.location
            .as_ref()
            .map(|location| location.range.start.line),
        Some(62)
    );

    let _ = session.resume(None).await;
    let _ = session.detach().await;
}

#[tokio::test]
#[ignore = "requires a JDK"]
async fn a_thread_that_is_not_suspended_keeps_running_after_detach() {
    let Some(java) = jdk_executable("java") else {
        eprintln!("nenhum JDK encontrado; teste ignorado");
        return;
    };
    let root = std::env::temp_dir().join(format!("er-ide-detach-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let package = root.join("src/main/java/com/example");
    assert!(fs::create_dir_all(&package).is_ok());
    let source = package.join("Loop.java");
    assert!(fs::write(&source, PROGRAM).is_ok());
    let classes = root.join("classes");
    let Some(javac) = jdk_executable("javac") else {
        return;
    };
    let _ = Command::new(javac)
        .arg("-g")
        .arg("-d")
        .arg(&classes)
        .arg(&source)
        .output();

    let port = free_port().await;
    let Ok(process) = Command::new(&java)
        .arg(format!(
            "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=127.0.0.1:{port}"
        ))
        .arg("-cp")
        .arg(&classes)
        .arg("com.example.Loop")
        .spawn()
    else {
        return;
    };
    let mut jvm = Jvm { process };

    let sink = Arc::new(RecordingSink::default());
    let adapter = JavaDebugAdapter::new();
    let mut attached = None;
    for _ in 0..40 {
        let request = DebugSessionRequest::new(DebugTarget::new("127.0.0.1", port))
            .with_source_roots(vec![root.join("src/main/java")]);
        match adapter
            .attach(request, Arc::clone(&sink) as Arc<dyn DebugEventSink>)
            .await
        {
            Ok(session) => {
                attached = Some(session);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    let Some(session) = attached else {
        panic!("não foi possível conectar à JVM em 127.0.0.1:{port}");
    };

    assert!(session.detach().await.is_ok());
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        matches!(jvm.process.try_wait(), Ok(None)),
        "o processo depurado deve continuar executando após a desconexão"
    );
    let _ = fs::remove_dir_all(root);
}
