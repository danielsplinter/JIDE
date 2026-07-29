//! Fluxo completo do adapter contra um alvo que fala o protocolo de verdade.
//!
//! O alvo falso responde no socket como uma JVM com depuração habilitada, o que
//! exercita handshake, negociação de larguras, instalação de breakpoint, evento
//! de parada, leitura de pilha, variáveis e passo a passo — sem exigir uma JVM
//! instalada para rodar os testes.

use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use ide_debug_api::{
    DebugAdapter, DebugEvent, DebugEventSink, DebugSessionRequest, DebugTarget, FrameId,
    SourceBreakpoint, StepKind, StopReason, ThreadId,
};
use java_debug_adapter::JavaDebugAdapter;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
};

const HANDSHAKE: &[u8] = b"JDWP-Handshake";
const THREAD: u64 = 500;
const CLASS: u64 = 100;
static NEXT_WORKSPACE: AtomicUsize = AtomicUsize::new(1);
const METHOD: u64 = 200;
const FRAME: u64 = 300;

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

    async fn wait_for(&self, predicate: impl Fn(&DebugEvent) -> bool) -> DebugEvent {
        for _ in 0..50 {
            if let Some(event) = self.events().into_iter().find(|event| predicate(event)) {
                return event;
            }
            let _ = tokio::time::timeout(Duration::from_millis(200), self.notify.notified()).await;
        }
        panic!("expected debug event never arrived: {:?}", self.events());
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

// ------------------------------------------------------------ alvo simulado

#[derive(Default)]
struct Payload(Vec<u8>);

impl Payload {
    fn u8(mut self, value: u8) -> Self {
        self.0.push(value);
        self
    }
    fn i32(mut self, value: i32) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }
    fn i64(mut self, value: i64) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }
    fn id(self, value: u64) -> Self {
        self.i64(value as i64)
    }
    fn string(mut self, value: &str) -> Self {
        self.0
            .extend_from_slice(&(value.len() as i32).to_be_bytes());
        self.0.extend_from_slice(value.as_bytes());
        self
    }
    fn location(self) -> Self {
        self.u8(1).id(CLASS).id(METHOD).i64(5)
    }
}

struct Request {
    id: u32,
    command_set: u8,
    command: u8,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut header = [0_u8; 11];
    stream.read_exact(&mut header).await.ok()?;
    let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let id = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let mut body = vec![0_u8; length.saturating_sub(11)];
    if !body.is_empty() {
        stream.read_exact(&mut body).await.ok()?;
    }
    Some(Request {
        id,
        command_set: header[9],
        command: header[10],
        body,
    })
}

async fn write_reply(stream: &mut TcpStream, id: u32, payload: &[u8]) {
    let mut packet = ((11 + payload.len()) as u32).to_be_bytes().to_vec();
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x80, 0, 0]);
    packet.extend_from_slice(payload);
    let _ = stream.write_all(&packet).await;
}

async fn write_event(stream: &mut TcpStream, payload: &[u8]) {
    let mut packet = ((11 + payload.len()) as u32).to_be_bytes().to_vec();
    packet.extend_from_slice(&0_u32.to_be_bytes());
    packet.extend_from_slice(&[0, 64, 100]);
    packet.extend_from_slice(payload);
    let _ = stream.write_all(&packet).await;
}

/// Responde como uma JVM parada em `com.example.Main.main`, linha 12.
async fn fake_vm(listener: TcpListener, mut trigger: oneshot::Receiver<()>) {
    let Ok((mut stream, _)) = listener.accept().await else {
        return;
    };
    let mut greeting = [0_u8; HANDSHAKE.len()];
    if stream.read_exact(&mut greeting).await.is_err() || greeting != HANDSHAKE {
        return;
    }
    let _ = stream.write_all(HANDSHAKE).await;

    let mut next_request = 0_i32;
    while let Some(request) = read_request(&mut stream).await {
        let reply = match (request.command_set, request.command) {
            // VirtualMachine.IDSizes
            (1, 7) => Payload::default().i32(8).i32(8).i32(8).i32(8).i32(8),
            // VirtualMachine.Version
            (1, 1) => Payload::default()
                .string("FakeVM 1.8")
                .i32(1)
                .i32(8)
                .string("1.8.0")
                .string("FakeVM"),
            // VirtualMachine.AllClasses
            (1, 3) => Payload::default()
                .i32(1)
                .u8(1)
                .id(CLASS)
                .string("Lcom/example/Main;")
                .i32(7),
            // VirtualMachine.AllThreads
            (1, 4) => Payload::default().i32(1).id(THREAD),
            // ReferenceType.Signature
            (2, 1) => Payload::default().string("Lcom/example/Main;"),
            // ReferenceType.Methods
            (2, 5) => Payload::default()
                .i32(1)
                .id(METHOD)
                .string("main")
                .string("([Ljava/lang/String;)V")
                .i32(9),
            // Method.LineTable
            (6, 1) => Payload::default()
                .i64(0)
                .i64(20)
                .i32(2)
                .i64(0)
                .i32(10)
                .i64(5)
                .i32(12),
            // Method.VariableTable
            (6, 2) => Payload::default()
                .i32(1)
                .i32(1)
                .i64(0)
                .string("total")
                .string("I")
                .i32(20)
                .i32(1),
            // ThreadReference.Name
            (11, 1) => Payload::default().string("main"),
            // ThreadReference.Status
            (11, 4) => Payload::default().i32(2).i32(1),
            // ThreadReference.Frames
            (11, 6) => Payload::default().i32(1).id(FRAME).location(),
            // StackFrame.GetValues
            (16, 1) => Payload::default().i32(1).u8(b'I').i32(7),
            // StackFrame.ThisObject
            (16, 3) => Payload::default().u8(b'L').id(0),
            // EventRequest.Set
            (15, 1) => {
                next_request += 1;
                Payload::default().i32(next_request)
            }
            _ => Payload::default(),
        };
        let is_breakpoint_request =
            request.command_set == 15 && request.command == 1 && request.body.first() == Some(&2);
        write_reply(&mut stream, request.id, &reply.0).await;

        if is_breakpoint_request {
            // O teste dispara a parada quando termina de instalar o breakpoint.
            let _ = (&mut trigger).await;
            let composite = Payload::default()
                .u8(1)
                .i32(1)
                .u8(2)
                .i32(next_request)
                .id(THREAD)
                .location();
            write_event(&mut stream, &composite.0).await;
        }
    }
}

fn workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "er-ide-jdwp-{}-{}",
        std::process::id(),
        NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    let package = root.join("src/main/java/com/example");
    assert!(fs::create_dir_all(&package).is_ok());
    let source = (1..=14)
        .map(|line| format!("// linha {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(fs::write(package.join("Main.java"), source).is_ok());
    root
}

#[tokio::test]
async fn attaches_stops_on_a_breakpoint_and_walks_the_stack() {
    let root = workspace();
    let source_root = root.join("src/main/java");
    let main_java = source_root.join("com/example/Main.java");

    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) => panic!("could not bind the fake target: {error}"),
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => panic!("could not read the fake target address: {error}"),
    };
    let (trigger, wait_for_trigger) = oneshot::channel();
    tokio::spawn(fake_vm(listener, wait_for_trigger));

    let sink = Arc::new(RecordingSink::default());
    let adapter = JavaDebugAdapter::new();
    let session = match adapter
        .attach(
            DebugSessionRequest::new(DebugTarget::new(address.ip().to_string(), address.port()))
                .with_source_roots(vec![source_root.clone()]),
            Arc::clone(&sink) as Arc<dyn DebugEventSink>,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => panic!("attach failed: {error}"),
    };

    assert!(matches!(
        sink.wait_for(|event| matches!(event, DebugEvent::Attached { .. })).await,
        DebugEvent::Attached { description } if description.contains("FakeVM")
    ));

    // Linha 9 (0-based) é a linha 10 do arquivo, presente na tabela de linhas.
    let resolved = match session
        .set_breakpoints(&main_java, &[SourceBreakpoint::new(&main_java, 9)])
        .await
    {
        Ok(resolved) => resolved,
        Err(error) => panic!("set_breakpoints failed: {error}"),
    };
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].verified_line, Some(9));
    assert!(resolved[0].message.is_none());

    let _ = trigger.send(());
    let stopped = sink
        .wait_for(|event| matches!(event, DebugEvent::Stopped { .. }))
        .await;
    assert!(matches!(
        stopped,
        DebugEvent::Stopped { thread, reason: StopReason::Breakpoint(id) }
            if thread == ThreadId(THREAD) && id == resolved[0].id
    ));

    let frames = match session.stack_trace(ThreadId(THREAD)).await {
        Ok(frames) => frames,
        Err(error) => panic!("stack_trace failed: {error}"),
    };
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].name, "Main.main");
    assert_eq!(frames[0].id, FrameId(FRAME));
    let location = match frames[0].location.as_ref() {
        Some(location) => location,
        None => panic!("frame has no source location"),
    };
    assert_eq!(location.path, main_java);
    assert_eq!(
        location.range.start.line, 11,
        "índice 5 corresponde à linha 12 do arquivo, 0-based 11"
    );

    let variables = match session.variables(ThreadId(THREAD), FrameId(FRAME)).await {
        Ok(variables) => variables,
        Err(error) => panic!("variables failed: {error}"),
    };
    assert_eq!(variables.len(), 1, "método estático não expõe `this`");
    assert_eq!(variables[0].name, "total");
    assert_eq!(variables[0].value, "7");
    assert!(!variables[0].expandable);

    let evaluated = match session
        .evaluate(ThreadId(THREAD), FrameId(FRAME), "total")
        .await
    {
        Ok(evaluated) => evaluated,
        Err(error) => panic!("evaluate failed: {error}"),
    };
    assert_eq!(evaluated.value, "7");
    assert!(
        session
            .evaluate(ThreadId(THREAD), FrameId(FRAME), "total.size()")
            .await
            .is_err(),
        "chamar métodos executaria código no alvo e é recusado"
    );

    assert!(session.step(ThreadId(THREAD), StepKind::Over).await.is_ok());
    assert!(matches!(
        sink.wait_for(|event| matches!(event, DebugEvent::Resumed { .. })).await,
        DebugEvent::Resumed { thread } if thread == ThreadId(THREAD)
    ));

    assert!(session.detach().await.is_ok());
    assert!(matches!(
        sink.wait_for(|event| matches!(event, DebugEvent::Detached { .. }))
            .await,
        DebugEvent::Detached { .. }
    ));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn breakpoints_outside_the_project_roots_are_reported_unverified() {
    let root = workspace();
    let source_root = root.join("src/main/java");

    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) => panic!("could not bind the fake target: {error}"),
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => panic!("could not read the fake target address: {error}"),
    };
    let (_trigger, wait_for_trigger) = oneshot::channel();
    tokio::spawn(fake_vm(listener, wait_for_trigger));

    let sink = Arc::new(RecordingSink::default());
    let session = match JavaDebugAdapter::new()
        .attach(
            DebugSessionRequest::new(DebugTarget::new(address.ip().to_string(), address.port()))
                .with_source_roots(vec![source_root]),
            Arc::clone(&sink) as Arc<dyn DebugEventSink>,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => panic!("attach failed: {error}"),
    };

    let outside = root.join("scripts/Helper.java");
    let resolved = match session
        .set_breakpoints(&outside, &[SourceBreakpoint::new(&outside, 3)])
        .await
    {
        Ok(resolved) => resolved,
        Err(error) => panic!("set_breakpoints failed: {error}"),
    };
    assert_eq!(resolved.len(), 1);
    assert!(!resolved[0].is_verified());
    assert!(
        resolved[0]
            .message
            .as_deref()
            .is_some_and(|message| message.contains("raízes de código"))
    );

    let _ = session.detach().await;
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn attaching_to_a_closed_port_fails_without_hanging() {
    let sink = Arc::new(RecordingSink::default());
    let result = JavaDebugAdapter::new()
        .attach(
            DebugSessionRequest::new(DebugTarget::new("127.0.0.1", 1)),
            sink as Arc<dyn DebugEventSink>,
        )
        .await;
    assert!(result.is_err());
}
