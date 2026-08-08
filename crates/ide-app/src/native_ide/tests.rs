//! Os testes da raiz de composição.
//!
//! Eles moravam no fim do `native_ide.rs`, e eram metade do arquivo: o teto de
//! linhas dele foi levantado seis vezes, e boa parte de cada subida era teste
//! novo, não produto novo. Separados, o teto passa a medir o que ele quis medir
//! desde o começo — o tamanho da raiz de composição.
//!
//! A raiz é um binário, e binário não tem `lib`: um teste em `tests/` não
//! alcançaria `NativeIde` nem os campos dela. Por isso é um módulo de teste do
//! próprio arquivo, e não um teste de integração.

use ide_project::model::{
    BuildSystemId, ModuleId, ProjectDescriptor, ProjectModule, SourceRoots,
};
use ide_workspace::WorkspaceService;

use super::*;

/// Duas margens pedidas em sequência chegam as duas.
///
/// Elas passavam por um controlador que **cancela a anterior** — o mesmo das
/// buscas, onde só a última resposta interessa. Aqui cada resposta é a
/// margem de um arquivo: cancelar a de A ao abrir B deixava A sem marca
/// nenhuma até alguém gravar. Agora é uma fila, e o teste guarda isso.
#[test]
fn as_margens_pedidas_em_sequencia_chegam_as_duas() {
    let mut fila: Vec<std::sync::mpsc::Receiver<GitDiffOutcome>> = Vec::new();
    let mut mandar = |caminho: &str| {
        let (envio, recepcao) = std::sync::mpsc::channel();
        fila.push(recepcao);
        let _ = envio.send(GitDiffOutcome {
            path: PathBuf::from(caminho),
            committed: String::new(),
            marks: vec![(0, ide_ui::GitLineChange::Added)],
            removed: Vec::new(),
            added_spans: Vec::new(),
            removed_spans: Vec::new(),
            pairs: Vec::new(),
            staged: false,
            comparar: false,
            error: None,
        });
    };
    mandar("Um.java");
    mandar("Dois.java");

    // A coleta leva **todas** as que chegaram, e não a primeira: guardar uma
    // para o quadro seguinte faria a margem de um arquivo esperar pela de
    // outro.
    let mut chegaram = Vec::new();
    fila.retain(|receptor| match receptor.try_recv() {
        Ok(resultado) => {
            chegaram.push(resultado.path);
            false
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => true,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
    });
    assert_eq!(
        chegaram,
        vec![PathBuf::from("Um.java"), PathBuf::from("Dois.java")],
        "as duas respostas, na ordem em que foram pedidas"
    );
    assert!(fila.is_empty(), "e a fila esvazia");
}

fn test_shell(root: &Path) -> IdeShell {
    let service = WorkspaceService::native();
    match service.scan(root) {
        Ok(tree) => IdeShell::from_tree(tree),
        Err(error) => panic!("projeto não abriu: {error}"),
    }
}

fn open_test_document(shell: &mut IdeShell, path: &Path) -> DocumentId {
    let service = WorkspaceService::native();
    match service.read_document(path) {
        Ok(text) => shell.show_document(path, text),
        Err(error) => panic!("documento não abriu: {error}"),
    }
}

/// Ctrl+clique encontra a definição em outro arquivo do projeto, para
/// qualquer forma de declarar um tipo.
///
/// `record` não estava no índice: navegar até um DTO — a forma mais comum
/// de declarar um no Java moderno — não encontrava nada, enquanto classes e
/// interfaces funcionavam.
#[test]
fn navigation_finds_definitions_declared_in_other_files() {
    let root = std::env::temp_dir().join(format!("er-ide-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let pacote = root.join("src");
    assert!(std::fs::create_dir_all(&pacote).is_ok());
    assert!(
        std::fs::write(
            pacote.join("Pedido.java"),
            "public record Pedido(String v) {}
"
        )
        .is_ok()
    );
    assert!(
        std::fs::write(
            pacote.join("Servico.java"),
            "public interface Servico {}
"
        )
        .is_ok()
    );
    assert!(
        std::fs::write(
            pacote.join("Estado.java"),
            "public enum Estado { ATIVO }
"
        )
        .is_ok()
    );
    assert!(
        std::fs::write(
            pacote.join("Ajuda.java"),
            "public class Ajuda {}
"
        )
        .is_ok()
    );
    let uso = pacote.join("Uso.java");
    let texto = "public class Uso { void f() { Pedido p; Servico s; Estado e; Ajuda a; } }
";
    assert!(std::fs::write(&uso, texto).is_ok());

    let language_host = LanguageHost::new(&root);
    let java = java_contribution::contribution(Arc::new(NativeProcessSupervisor::default()));
    assert!(language_host.register(java.provider.clone()).is_ok());
    let mut ide = NativeIde::default();
    // A contribuição, e não só o provider: é dela que sai a lista de
    // extensões que a sincronização consulta. Ver a fase 1b da `23`.
    assert!(ide.languages.contributions.register(java).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));
    let document_id = match ide.ui.shell.as_mut() {
        Some(shell) => open_test_document(shell, &uso),
        None => panic!("shell de teste ausente"),
    };
    ide.sync_languages();
    // Ativar não espera mais o índice: quem afirma a navegação pelo projeto
    // inteiro precisa dele pronto. Ver a fase 2 da `19`.
    if let Some(host) = &ide.languages.host {
        assert!(
            pollster::block_on(host.wait_until_indexed(std::time::Duration::from_secs(60)))
                .unwrap_or(false),
            "o índice do projeto não ficou pronto a tempo"
        );
    }

    for (token, arquivo) in [
        ("Pedido", "Pedido.java"),
        ("Servico", "Servico.java"),
        ("Estado", "Estado.java"),
        ("Ajuda", "Ajuda.java"),
    ] {
        let byte_offset = texto.find(token).unwrap_or_default();
        ide.navigate_to_definition(NavigationRequest {
            document_id,
            byte_offset,
            token: token.to_owned(),
        });
        // A navegação deixou de ser síncrona na fase 5 da `25`: perguntar ao
        // analisador custa o que ele demorar, e esperar por isso na chamada
        // é a janela parada. Quem recolhe é o laço de quadros, e aqui o teste
        // faz o papel dele.
        let mut esperas = 0;
        while !ide.collect_navigation() {
            std::thread::sleep(std::time::Duration::from_millis(10));
            esperas += 1;
            assert!(esperas < 500, "a navegação precisa terminar");
        }
        let mensagem = ide
            .ui
            .shell
            .as_ref()
            .map(|shell| shell.status_message().to_owned())
            .unwrap_or_default();
        assert!(
            mensagem.contains(arquivo),
            "{token} deveria levar a {arquivo}, veio: {mensagem}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// **Escrever um nome abre a lista, com os dois providers no caminho.**
///
/// # Por que este teste é no `ide-app`, e não na crate da linguagem
///
/// Os dois providers já foram sondados em separado, e os dois respondem:
/// o nativo diz `Unresolved` — "não é comigo" — e o analisador devolve
/// `HttpClient` e companhia. Mesmo assim, `private http: Http` não
/// completava na tela.
///
/// **O que faltava testar era o caminho**: a tecla, o disparo na segunda
/// letra, o roteamento do host entre os dois candidatos, e o recolhimento
/// da resposta no quadro seguinte. Nenhuma peça sozinha explicava a falha,
/// e é essa a razão de o teste morar aqui.
///
/// ```text
/// ER_IDE_PROJETO_TS=C:/caminho/do/projeto cargo test --release -p ide-app -- --ignored --nocapture escrever_um_nome
/// ```
#[test]
#[ignore = "exige ER_IDE_PROJETO_TS com node_modules instalado"]
fn typing_a_name_opens_the_list_through_both_providers() {
    let Ok(raiz) = std::env::var("ER_IDE_PROJETO_TS") else {
        panic!("aponte ER_IDE_PROJETO_TS para um projeto TypeScript");
    };
    let raiz = PathBuf::from(raiz);
    let arquivo = raiz.join("src/app/er-teste-nome.ts");
    let codigo = "import { HttpClient } from '@angular/common/http';\n\
                  export class Pagina {\n  constructor(private c: ) {}\n}\n";
    assert!(std::fs::write(&arquivo, codigo).is_ok());

    let processos = Arc::new(NativeProcessSupervisor::default());
    let language_host = LanguageHost::new(&raiz);
    let typescript = typescript_contribution::contribution(processos.clone(), &[]);
    assert!(language_host.register(typescript.provider.clone()).is_ok());
    assert!(
        language_host
            .register(typescript_contribution::service_provider(processos, Vec::new()))
            .is_ok()
    );
    assert!(
        language_host
            .configure_selection(
                ide_domain::LanguageId("typescript".to_owned()),
                typescript_contribution::selection(),
            )
            .is_ok()
    );
    let mut ide = NativeIde::default();
    assert!(ide.languages.contributions.register(typescript).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&raiz));
    let _ = match ide.ui.shell.as_mut() {
        Some(shell) => open_test_document(shell, &arquivo),
        None => panic!("shell de teste ausente"),
    };
    ide.sync_languages();
    if let Some(host) = &ide.languages.host {
        assert!(
            pollster::block_on(host.wait_until_indexed(std::time::Duration::from_secs(300)))
                .unwrap_or(false),
            "o projeto precisa ficar pronto"
        );
    }

    // O cursor logo depois de `private c: `, e as duas letras digitadas
    // **pelo caminho da tecla** — que é o que faltava exercitar.
    if let Some(shell) = ide.ui.shell.as_mut() {
        shell.show_location(&arquivo, codigo, 2, 25);
    }
    assert!(!ide.text_typed("H"), "uma letra não pede a lista");
    assert!(ide.text_typed("t"), "duas letras pedem a lista");
    ide.sync_languages();
    ide.request_completion();

    let mut esperas = 0;
    loop {
        ide.collect_completion();
        if ide
            .ui
            .shell
            .as_ref()
            .is_some_and(ide_ui::IdeShell::completion_open)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        esperas += 1;
        assert!(
            esperas < 400,
            "a lista precisa abrir escrevendo um nome; \
             os dois providers respondem em separado, e o caminho é que falhava"
        );
    }
    let _ = std::fs::remove_file(&arquivo);
}

/// **O ponto não espera pela resposta.**
///
/// Era o sexto lugar com o mesmo defeito, e o mais bem escondido: a
/// completação era pedida na chamada, com o prazo de cinco segundos do
/// analisador, e a tela ficava parada nesse tempo. Quem digitava depois do
/// ponto via as letras aparecerem todas de uma vez no fim — elas ficavam na
/// fila da janela, porque nenhum quadro era desenhado.
///
/// O que se afirma aqui é o que faltava: a tecla **posta** a pergunta e
/// volta, e a resposta encontra a tela num quadro seguinte.
#[test]
fn the_dot_does_not_wait_for_the_answer() {
    let root = std::env::temp_dir().join(format!("er-ide-completacao-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let alvo = root.join("pedido.ts");
    let texto = "export class Pedido {\n  total = 0;\n  somar(v: number) {}\n  usar() {\n    this.\n  }\n}\n";
    assert!(std::fs::write(&alvo, texto).is_ok());

    let language_host = LanguageHost::new(&root);
    let typescript = typescript_contribution::contribution(
        Arc::new(NativeProcessSupervisor::default()),
        &[],
    );
    assert!(language_host.register(typescript.provider.clone()).is_ok());
    let mut ide = NativeIde::default();
    assert!(ide.languages.contributions.register(typescript).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));
    if let Some(shell) = ide.ui.shell.as_mut() {
        // Logo depois do `this.`, que é onde a lista deve abrir.
        shell.show_location(&alvo, texto, 4, 9);
    }
    ide.sync_languages();

    let inicio = std::time::Instant::now();
    ide.request_completion();
    let gasto = inicio.elapsed();

    assert!(
        ide.languages.completion.pending.is_some(),
        "a pergunta ficou pendente em vez de ser esperada"
    );
    // Folgado de propósito: o que se afirma é que a tecla não paga a
    // resposta, e não quanto a máquina que roda o teste é rápida.
    assert!(
        gasto < std::time::Duration::from_millis(30),
        "o ponto custou {gasto:?}, como se ainda esperasse a resposta"
    );

    // E a resposta chega, ainda que depois — recolhida pelo laço de quadros,
    // cujo papel o teste faz aqui.
    let mut esperas = 0;
    loop {
        ide.collect_completion();
        let aberta = ide
            .ui
            .shell
            .as_ref()
            .is_some_and(ide_ui::IdeShell::completion_open);
        if aberta {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        esperas += 1;
        assert!(esperas < 500, "a completação precisa chegar");
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// Digitar não espera pela análise da linguagem.
///
/// Era o que travava a digitação: a tecla ficava presa esperando o provider
/// analisar o arquivo — mais de 400 ms num arquivo grande, e ainda 60 ms
/// depois de a análise emagrecer. O provider sempre teve thread própria; o
/// que faltava era não esperar por ela.
#[test]
fn typing_does_not_wait_for_the_language_analysis() {
    let root = std::env::temp_dir().join(format!("er-ide-async-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let pacote = root.join("src");
    assert!(std::fs::create_dir_all(&pacote).is_ok());
    // Grande o bastante para a análise não caber num quadro.
    let corpo: String = (0..3000)
        .map(|indice| {
            format!(
                "    int metodo{indice}() {{ return {indice}; }}
"
            )
        })
        .collect();
    let alvo = pacote.join("Grande.java");
    assert!(
        std::fs::write(
            &alvo,
            format!(
                "public class Grande {{
{corpo}}}
"
            )
        )
        .is_ok()
    );

    let language_host = LanguageHost::new(&root);
    let java = java_contribution::contribution(Arc::new(NativeProcessSupervisor::default()));
    assert!(language_host.register(java.provider.clone()).is_ok());
    let mut ide = NativeIde::default();
    // A contribuição, e não só o provider: é dela que sai a lista de
    // extensões que a sincronização consulta. Ver a fase 1b da `23`.
    assert!(ide.languages.contributions.register(java).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));
    if let Some(shell) = ide.ui.shell.as_mut() {
        open_test_document(shell, &alvo);
    }
    ide.sync_languages();
    ide.settle_syntax();

    // Uma tecla, e o tempo que ela custa no laço da janela.
    if let Some(shell) = ide.ui.shell.as_mut() {
        shell.text_input("a");
    }
    let inicio = std::time::Instant::now();
    ide.sync_languages();
    let gasto = inicio.elapsed();

    assert!(
        ide.languages.pending_syntax() > 0,
        "a tecla deixou a análise pendente em vez de esperar por ela"
    );
    // Folgado de propósito: o que se afirma é que a tecla não paga a análise,
    // e não quanto a máquina que roda o teste é rápida.
    assert!(
        gasto < std::time::Duration::from_millis(30),
        "a tecla custou {gasto:?}, como se ainda esperasse a análise"
    );

    // E o realce chega, ainda que depois.
    ide.settle_syntax();
    assert_eq!(ide.languages.pending_syntax(), 0);
    let realcado = ide
        .ui
        .shell
        .as_ref()
        .and_then(|shell| {
            shell
                .syntax_snapshot(DocumentId(1))
                .map(|s| s.highlights.len())
        })
        .unwrap_or_default();
    assert!(realcado > 0, "o realce chegou pela thread do provider");

    let _ = std::fs::remove_dir_all(&root);
}

/// O código já aparece colorido no primeiro quadro.
///
/// O realce era pedido só dentro do tratamento de eventos, então o texto
/// ficava sem cor até o primeiro clique ou troca de aba.
#[test]
fn the_code_is_highlighted_before_the_first_interaction() {
    let root = std::env::temp_dir().join(format!("er-ide-realce-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let file = root.join("Exemplo.java");
    assert!(std::fs::write(&file, "public class Exemplo {}").is_ok());

    let language_host = LanguageHost::new(&root);
    let java = java_contribution::contribution(Arc::new(NativeProcessSupervisor::default()));
    assert!(language_host.register(java.provider.clone()).is_ok());

    let mut ide = NativeIde::default();
    // A contribuição precisa estar registrada, e não só o provider: é dela
    // que sai a lista de extensões que a sincronização de documentos
    // consulta. Ver a fase 1b da `23`.
    assert!(ide.languages.contributions.register(java).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));
    if let Some(shell) = ide.ui.shell.as_mut() {
        open_test_document(shell, &file);
    }

    let keyword_colored = |ide: &mut NativeIde| {
        let colors = ui_core::Theme::default().colors;
        ide.ui
            .shell
            .as_mut()
            .map(|shell| shell.paint(Size::new(1_280.0, 800.0)))
            .unwrap_or_default()
            .iter()
            .any(|command| {
                matches!(
                    command,
                    ui_render_api::PaintCommand::DrawText(text)
                        if text.text == "public" && text.color == colors.syntax_keyword
                )
            })
    };
    assert!(
        !keyword_colored(&mut ide),
        "sem o realce pedido, o texto sai sem cor"
    );

    // É isto que a inicialização passou a fazer. O realce vem da thread do
    // provider, então o teste espera por ele; na janela, quem o recolhe é o
    // relógio, uns 30 ms depois — sem a digitação ter esperado.
    ide.sync_languages();
    ide.settle_syntax();
    assert!(
        keyword_colored(&mut ide),
        "depois de pedir o realce, o código aparece colorido"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// Um `.ts` aberto na IDE aparece colorido, como um `.java`.
///
/// É o critério da fase 1 da `23` cobrado onde ele vale: no caminho da
/// **aplicação**. O teste que deu a fase por cumprida montava um
/// `LanguageHost` e falava com ele — e o host roteava certo o tempo todo. O
/// que descartava o `.ts` era a sincronização de documentos, um nível acima,
/// A linguagem do recente sai da **pasta**, e não do projeto ainda aberto.
///
/// Relatado por quem usa: um projeto Java apareceu debaixo de "TypeScript".
/// Ele foi aberto vindo de um projeto TypeScript, e o registro perguntava ao
/// `project.imported` — que só é trocado dentro da importação, depois do
/// primeiro quadro. A pasta nova era etiquetada com a linguagem da anterior,
/// e a correção só chegava se a importação desse certo.
#[test]
fn a_linguagem_do_recente_e_a_da_pasta_registrada() {
    let raiz = std::env::temp_dir().join(format!("er-ide-recente-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    let java = raiz.join("camel");
    assert!(std::fs::create_dir_all(&java).is_ok());
    assert!(
        std::fs::write(
            java.join("pom.xml"),
            "<project><artifactId>camel</artifactId></project>",
        )
        .is_ok()
    );
    let qualquer = raiz.join("rascunho");
    assert!(std::fs::create_dir_all(&qualquer).is_ok());

    let processes = Arc::new(NativeProcessSupervisor::default());
    let mut ide = NativeIde::default();
    ide.runtime.processes = Some(processes.clone());
    ide.register_build_systems();
    assert!(
        ide.languages
            .contributions
            .register(java_contribution::contribution(processes.clone()))
            .is_ok()
    );
    assert!(
        ide.languages
            .contributions
            .register(typescript_contribution::contribution(processes, &[]))
            .is_ok()
    );
    // O projeto anterior continua importado: é exatamente a situação de
    // quem troca de projeto, porque a importação do novo ainda não rodou.
    ide.project.imported = Some(ImportedProject {
        adapter: match ide.project.build_systems.adapter(&BuildSystemId(
            language_typescript::NPM_BUILD_SYSTEM_ID.to_owned(),
        )) {
            Some(adapter) => adapter,
            None => panic!("o sistema de build do projeto anterior precisa estar registrado"),
        },
        descriptor: ProjectDescriptor {
            build_system: BuildSystemId(language_typescript::NPM_BUILD_SYSTEM_ID.to_owned()),
            root: raiz.join("loja"),
            manifest: raiz.join("loja").join("package.json"),
            name: None,
            wrapper: None,
        },
        model: ProjectModel::new(
            BuildSystemId(language_typescript::NPM_BUILD_SYSTEM_ID.to_owned()),
            raiz.join("loja"),
            "loja",
        ),
        manifest_modified: None,
    });

    assert_eq!(
        ide.detected_language(&java).as_deref(),
        Some(java_contribution::JAVA_LANGUAGE_ID),
        "a pasta tem manifesto de Java, e é dela que a resposta sai"
    );
    assert_eq!(
        ide.detected_language(&qualquer),
        None,
        "uma pasta que não é projeto de ninguém não recebe linguagem inventada"
    );

    // O formato de quem clona um repositório dentro de uma pasta de mesmo
    // nome: a raiz aberta não tem manifesto, e o projeto começa um nível
    // abaixo. Foi assim que um projeto Java deixou de ser reconhecido.
    let embrulho = raiz.join("embrulho");
    assert!(std::fs::create_dir_all(embrulho.join("camel")).is_ok());
    assert!(
        std::fs::write(
            embrulho.join("camel").join("pom.xml"),
            "<project><artifactId>camel</artifactId></project>",
        )
        .is_ok()
    );
    assert_eq!(
        ide.detected_language(&embrulho).as_deref(),
        Some(java_contribution::JAVA_LANGUAGE_ID),
        "a pasta que só embrulha o projeto responde pelo que há dentro dela"
    );
    let _ = std::fs::remove_dir_all(&raiz);
}

/// que perguntava se a extensão era `java` com a palavra escrita à mão.
///
/// Testar a camada que se acabou de mexer e concluir sobre a de cima é o
/// defeito que este teste existe para não deixar voltar. Ver a fase 1b.
#[test]
fn a_typescript_file_is_highlighted_through_the_application() {
    let root = std::env::temp_dir().join(format!("er-ide-ts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let file = root.join("pedido.ts");
    assert!(std::fs::write(&file, "export class Pedido {}").is_ok());

    let language_host = LanguageHost::new(&root);
    let typescript = typescript_contribution::contribution(
        Arc::new(NativeProcessSupervisor::default()),
        &[],
    );
    assert!(language_host.register(typescript.provider.clone()).is_ok());

    let mut ide = NativeIde::default();
    assert!(ide.languages.contributions.register(typescript).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));
    if let Some(shell) = ide.ui.shell.as_mut() {
        open_test_document(shell, &file);
    }

    let keyword_colored = |ide: &mut NativeIde| {
        let colors = ui_core::Theme::default().colors;
        ide.ui
            .shell
            .as_mut()
            .map(|shell| shell.paint(Size::new(1_280.0, 800.0)))
            .unwrap_or_default()
            .iter()
            .any(|command| {
                matches!(
                    command,
                    ui_render_api::PaintCommand::DrawText(text)
                        if text.text == "export" && text.color == colors.syntax_keyword
                )
            })
    };

    ide.sync_languages();
    ide.settle_syntax();
    assert!(
        keyword_colored(&mut ide),
        "um `.ts` precisa chegar ao provider e voltar colorido"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// Devolver uma linha pela seta do diff não apaga a cor do editor.
///
/// A troca sobe a revisão do documento, e o realce guardado é o da revisão
/// anterior — a tela o descarta de propósito, senão coloriria as palavras
/// erradas. Quem devolve a linha tem de pedir realce novo: o realce do
/// clique é pedido durante o clique, e esta troca acontece depois dele.
#[test]
fn devolver_uma_linha_nao_deixa_o_arquivo_sem_cor() {
    let root = std::env::temp_dir().join(format!("er-ide-devolver-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let file = root.join("pedido.ts");
    assert!(std::fs::write(&file, "export class Pedido {}
").is_ok());

    let language_host = LanguageHost::new(&root);
    let typescript = typescript_contribution::contribution(
        Arc::new(NativeProcessSupervisor::default()),
        &[],
    );
    assert!(language_host.register(typescript.provider.clone()).is_ok());

    let mut ide = NativeIde::default();
    assert!(ide.languages.contributions.register(typescript).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));
    if let Some(shell) = ide.ui.shell.as_mut() {
        open_test_document(shell, &file);
    }

    let colorido = |ide: &mut NativeIde| {
        let colors = ui_core::Theme::default().colors;
        ide.ui
            .shell
            .as_mut()
            .map(|shell| shell.paint(Size::new(1_280.0, 800.0)))
            .unwrap_or_default()
            .iter()
            .any(|command| {
                matches!(
                    command,
                    ui_render_api::PaintCommand::DrawText(text)
                        if text.text == "export" && text.color == colors.syntax_keyword
                )
            })
    };

    ide.sync_languages();
    ide.settle_syntax();
    assert!(colorido(&mut ide), "o arquivo abre colorido");

    // A comparação carregada é o que dá à seta o texto de então.
    if let Some(shell) = ide.ui.shell.as_mut() {
        assert!(shell.abrir_comparacao(
            &file,
            ide_ui::GitDiff {
                current: "export class Pedido {}
".to_owned(),
                committed: "export class Compra {}
".to_owned(),
                removed: vec![0],
                ..ide_ui::GitDiff::default()
            },
        ));
    }
    ide.devolver_a_faixa(&file, (0, 1), (0, 1));
    ide.settle_syntax();

    assert_eq!(
        ide.ui.shell.as_ref().and_then(ide_ui::IdeShell::active_text),
        Some("export class Compra {}
"),
        "o editor mostra a linha devolvida"
    );
    assert!(
        colorido(&mut ide),
        "e continua colorido: a revisão subiu, e o realce veio atrás"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// As duas colunas da comparação saem coloridas, e não só o editor.
///
/// Um `diff` sem cor obriga a ler com mais atenção justamente onde a atenção já
/// está ocupada com o que mudou. Os dois lados não são abas — o arquivo de então
/// nem existe no disco —, e por isso entram na lista de documentos por conta
/// própria: é o que lhes dá realce, e é o que os fecha quando a janela sai.
#[test]
fn as_duas_colunas_da_comparacao_saem_coloridas() {
    let root = std::env::temp_dir().join(format!("er-ide-diff-cor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let file = root.join("pedido.ts");
    assert!(std::fs::write(&file, "export class Pedido {}
").is_ok());

    let language_host = LanguageHost::new(&root);
    let typescript =
        typescript_contribution::contribution(Arc::new(NativeProcessSupervisor::default()), &[]);
    assert!(language_host.register(typescript.provider.clone()).is_ok());

    let mut ide = NativeIde::default();
    assert!(ide.languages.contributions.register(typescript).is_ok());
    ide.languages.host = Some(Arc::new(language_host));
    ide.ui.shell = Some(test_shell(&root));

    // A janela do Git aberta, e uma comparação nela. O arquivo **não** está
    // aberto no editor: é o caso comum, o de clicar num arquivo alterado.
    if let Some(shell) = ide.ui.shell.as_mut() {
        shell.toggle_git();
        assert!(shell.abrir_comparacao(
            &file,
            ide_ui::GitDiff {
                committed: "export class Compra {}
".to_owned(),
                current: "export class Pedido {}
".to_owned(),
                ..ide_ui::GitDiff::default()
            },
        ));
    }
    ide.sync_languages();
    ide.settle_syntax();

    let colors = ui_core::Theme::default().colors;
    let quadro = ide
        .ui
        .shell
        .as_mut()
        .map(|shell| shell.paint(Size::new(1_280.0, 800.0)))
        .unwrap_or_default();
    let palavra_chave = |procurado: &str| {
        quadro.iter().any(|command| {
            matches!(
                command,
                ui_render_api::PaintCommand::DrawText(text)
                    if text.text == procurado && text.color == colors.syntax_keyword
            )
        })
    };
    // Duas vezes: uma por coluna. O `export` é palavra-chave dos dois lados.
    let vezes = quadro
        .iter()
        .filter(|command| {
            matches!(
                command,
                ui_render_api::PaintCommand::DrawText(text)
                    if text.text == "export" && text.color == colors.syntax_keyword
            )
        })
        .count();
    assert!(
        palavra_chave("export"),
        "a comparação sai colorida, e não em tinta única"
    );
    assert_eq!(vezes, 2, "os dois lados, e não só um");

    // E fechar a janela tira os dois documentos da lista: quem some daqui é
    // fechado do lado de quem analisa, e analisar o que ninguém olha é
    // trabalho jogado fora.
    if let Some(shell) = ide.ui.shell.as_mut() {
        shell.toggle_git();
        assert!(
            shell.document_snapshots().is_empty(),
            "sem janela e sem aba, não há documento nenhum a analisar"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// Ciclo completo: abrir abas, gravar, reabrir a IDE e encontrá-las de volta.
#[test]
fn the_open_tabs_come_back_with_the_project() {
    let root = std::env::temp_dir().join(format!("er-ide-abas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("projeto");
    assert!(std::fs::create_dir_all(&project).is_ok());
    let first = project.join("Primeiro.java");
    let second = project.join("Segundo.java");
    assert!(std::fs::write(&first, "class Primeiro {}").is_ok());
    assert!(std::fs::write(&second, "class Segundo {}").is_ok());
    let config_file = root.join("config.toml");

    // Sessão de trabalho: dois arquivos abertos pelo Explorer.
    let mut shell = test_shell(&project);
    open_test_document(&mut shell, &first);
    open_test_document(&mut shell, &second);

    let mut config = AppConfig::default();
    assert!(config.remember_workspace(&project, None, &config_file).is_ok());
    assert!(
        config
            .remember_documents(
                &shell.open_document_paths(),
                shell.active_document_path().as_deref(),
                &config_file,
            )
            .is_ok()
    );

    // Nova inicialização: a mesma restauração que `initialize` faz.
    let reopened = match AppConfig::load(&config_file) {
        Ok(config) => config,
        Err(error) => panic!("releitura falhou: {error}"),
    };
    let mut restored = test_shell(&project);
    assert_eq!(restored.tab_count(), 0, "a IDE abre sem abas");
    for document in reopened.workspace.resolved_documents(&project) {
        open_test_document(&mut restored, &document);
    }
    assert_eq!(restored.open_document_paths(), vec![first, second.clone()]);
    assert_eq!(restored.active_document_path(), Some(second));

    let _ = std::fs::remove_dir_all(&root);
}

/// O JDK e o Maven gravados no formato antigo sobrevivem à mudança.
///
/// É o risco principal da fase 0 da `23`: mexer em configuração persistida.
/// Uma migração malfeita apaga a escolha em silêncio, que é o pior jeito de
/// falhar — quem abre a IDE não distingue "nunca escolhi" de "perdi".
///
/// A tradução mora na raiz de composição porque saber que `jdk_home` era a
/// ferramenta principal de Java é conhecimento de linguagem, e o núcleo não
/// pode tê-lo.
#[test]
fn tools_chosen_in_the_old_format_survive_the_migration() {
    let root = std::env::temp_dir().join(format!("er-migracao-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let jdk = root.join("jdk-21");
    let maven = root.join("maven-3.9");
    assert!(std::fs::create_dir_all(&jdk).is_ok());
    assert!(std::fs::create_dir_all(&maven).is_ok());
    let config_file = root.join("config.toml");
    assert!(
        std::fs::write(
            &config_file,
            format!(
                "[toolchains]\njdk_home = {:?}\nmaven_home = {:?}\n",
                jdk.to_string_lossy(),
                maven.to_string_lossy()
            ),
        )
        .is_ok()
    );

    let Ok(mut config) = AppConfig::load(&config_file) else {
        panic!("configuração antiga precisa ser lida");
    };
    assert!(
        crate::bootstrap::migrate_legacy_toolchains(&mut config),
        "havia escolhas antigas a migrar"
    );

    let language = java_contribution::JAVA_LANGUAGE_ID;
    assert_eq!(
        config
            .toolchains
            .resolved(None, language, ToolRole::Primary)
            .map(|tool| tool.home),
        Some(jdk),
        "o JDK escolhido não pode se perder na mudança de formato"
    );
    assert_eq!(
        config
            .toolchains
            .resolved(None, language, ToolRole::Secondary)
            .map(|tool| tool.home),
        Some(maven)
    );

    // Migrar é uma vez só: gravado no formato novo, não há mais o que traduzir.
    assert!(config.save(&config_file).is_ok());
    let Ok(mut relido) = AppConfig::load(&config_file) else {
        panic!("configuração migrada precisa ser relida");
    };
    assert!(!crate::bootstrap::migrate_legacy_toolchains(&mut relido));
    assert!(
        relido
            .toolchains
            .resolved(None, language, ToolRole::Primary)
            .is_some(),
        "a escolha migrada continua valendo depois de regravada"
    );

    let _ = std::fs::remove_dir_all(&root);
}

fn maven_descriptor() -> ProjectDescriptor {
    ProjectDescriptor {
        build_system: BuildSystemId(MAVEN_BUILD_SYSTEM_ID.to_owned()),
        root: PathBuf::from("/w"),
        manifest: PathBuf::from("/w/pom.xml"),
        name: None,
        wrapper: None,
    }
}

fn model_with_roots() -> ProjectModel {
    let mut source_roots = SourceRoots::default();
    source_roots.push_main(PathBuf::from("/w/app/src/main/java"));
    source_roots.push_generated(PathBuf::from("/w/app/target/generated-sources/annotations"));
    let mut model = ProjectModel::new(
        BuildSystemId(MAVEN_BUILD_SYSTEM_ID.to_owned()),
        "/w",
        "demo",
    );
    model.modules.push(ProjectModule {
        id: ModuleId("app".to_owned()),
        name: "app".to_owned(),
        root: PathBuf::from("/w/app"),
        manifest: PathBuf::from("/w/app/pom.xml"),
        coordinates: None,
        packaging: "jar".to_owned(),
        source_roots,
        dependencies: Vec::new(),
        output_directory: PathBuf::from("/w/app/target/classes"),
        test_output_directory: PathBuf::from("/w/app/target/test-classes"),
        children: Vec::new(),
        plugins: Vec::new(),
    });
    model
}

#[test]
fn project_sources_keep_generated_code_and_drop_files_outside_the_model() {
    let files = vec![
        PathBuf::from("/w/app/src/main/java/Main.java"),
        PathBuf::from("/w/app/target/generated-sources/annotations/Generated.java"),
        PathBuf::from("/w/scripts/Helper.java"),
    ];
    let model = model_with_roots();

    let filtered = project_sources(files.clone(), Some(&model));
    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains(&PathBuf::from(
        "/w/app/target/generated-sources/annotations/Generated.java"
    )));
    assert!(!filtered.contains(&PathBuf::from("/w/scripts/Helper.java")));
}

#[test]
fn project_sources_fall_back_to_the_workspace_scan() {
    let files = vec![PathBuf::from("/w/scripts/Helper.java")];
    assert_eq!(project_sources(files.clone(), None), files);
    assert_eq!(
        project_sources(files.clone(), Some(&model_with_roots())),
        files,
        "um projeto sem fontes sob suas raízes não deve zerar a compilação"
    );
}

fn simbolo(nome: &str, caminho: &str) -> ide_domain::SemanticSymbol {
    ide_domain::SemanticSymbol {
        name: nome.to_owned(),
        kind: SymbolKind::Class,
        location: ide_domain::Location {
            path: PathBuf::from(caminho),
            range: TextRange::default(),
        },
        scope_depth: 0,
        type_descriptor: None,
    }
}

/// Uma consulta com hífen e uma em `CamelCase` são a mesma pergunta.
#[test]
fn a_hyphenated_query_and_a_camel_case_one_split_the_same() {
    assert_eq!(
        query_segments("federated-login-context"),
        vec!["federated", "login", "context"]
    );
    assert_eq!(
        query_segments("FederatedLoginContext"),
        vec!["federated", "login", "context"]
    );
    assert_eq!(query_segments("login"), vec!["login"]);
    assert_eq!(query_segments("  "), Vec::<String>::new());
}

/// Todos os pedaços precisam casar, e não qualquer um.
///
/// **Estes são os nomes que o analisador devolveu de verdade** para
/// `federated-login-context` numa sondagem: ele separa no hífen e responde
/// com o que casa com qualquer pedaço, incluindo `contextmenu` e
/// `AudioContext`, que vêm do `lib.dom.d.ts` e não do projeto. O casamento
/// exato vinha na décima segunda posição — foi o que se viu na tela.
#[test]
fn every_segment_of_the_query_has_to_match() {
    let raiz = PathBuf::from("/projeto");
    let vindos = vec![
        simbolo("context", "/projeto/src/a.ts"),
        simbolo("login", "/projeto/src/a.ts"),
        simbolo("contextmenu", "/lib/lib.dom.d.ts"),
        simbolo("AudioContext", "/lib/lib.dom.d.ts"),
        simbolo("ContextFederated", "/projeto/src/a.ts"),
        simbolo("FederatedAuthContext", "/projeto/src/a.ts"),
        simbolo("FederatedLoginContext", "/projeto/src/a.ts"),
        simbolo("LoginService", "/projeto/src/a.ts"),
    ];

    let refinados = refine_type_hits(vindos, "federated-login-context", Some(&raiz));
    let nomes: Vec<_> = refinados.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        nomes,
        vec!["FederatedLoginContext"],
        "só o que tem os três pedaços pode sobrar"
    );
}

/// Uma consulta de um pedaço só continua trazendo tudo o que casa.
///
/// **É como se busca em Java**, e é o que garante que a correção não estreitou
/// a busca das outras linguagens: com um pedaço só, exigir todos é exigir um.
#[test]
fn a_single_segment_query_keeps_everything_that_matches() {
    let raiz = PathBuf::from("/projeto");
    let vindos = vec![
        simbolo("LoginService", "/projeto/src/a.ts"),
        simbolo("FederatedLoginContext", "/projeto/src/a.ts"),
        simbolo("UserLoginToken", "/projeto/src/a.ts"),
        simbolo("PedidoDeCompra", "/projeto/src/a.ts"),
    ];
    let refinados = refine_type_hits(vindos, "login", Some(&raiz));
    let nomes: Vec<_> = refinados.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(nomes.len(), 3, "os três com `login` no nome ficam: {nomes:?}");
    assert!(!nomes.contains(&"PedidoDeCompra"));
}

/// O casamento melhor vem primeiro, e o de fora do projeto vem depois.
#[test]
fn the_best_match_comes_first_and_the_project_wins_ties() {
    let raiz = PathBuf::from("/projeto");
    let vindos = vec![
        simbolo("UserLoginToken", "/projeto/src/a.ts"),
        simbolo("NavigatorLogin", "/lib/lib.dom.d.ts"),
        simbolo("Login", "/projeto/src/a.ts"),
        simbolo("LoginService", "/projeto/src/a.ts"),
    ];
    let refinados = refine_type_hits(vindos, "login", Some(&raiz));
    let nomes: Vec<_> = refinados.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(nomes[0], "Login", "o casamento exato vem primeiro: {nomes:?}");
    assert_eq!(
        nomes.last(),
        Some(&"NavigatorLogin"),
        "o que não é do projeto desempata por último: {nomes:?}"
    );
}

/// Consulta vazia devolve tudo, que é o contrato da janela de busca.
#[test]
fn an_empty_query_keeps_everything() {
    let vindos = vec![simbolo("Qualquer", "/projeto/src/a.ts")];
    assert_eq!(refine_type_hits(vindos, "", None).len(), 1);
}

/// O projeto pedido ganha do último aberto e do diretório atual.
///
/// É o que sustenta "Duplicar workspace": a janela nova recebe a raiz como
/// argumento e não pergunta à configuração — que guarda o **último**
/// projeto, e pode já ser outro quando ela terminar de subir.
#[test]
fn o_projeto_pedido_ganha_de_todo_o_resto() {
    let pedido = PathBuf::from("/w/pedido");
    let mut config = AppConfig::default();
    config.workspace.last_path = Some(PathBuf::from("/w/ultimo"));

    assert_eq!(
        startup_root(
            Some(pedido.clone()),
            &config,
            Some(PathBuf::from("/w/atual"))
        ),
        Some(pedido),
        "o pedido vem antes do último projeto e do diretório atual"
    );
}

#[test]
fn startup_reopens_the_last_project_and_falls_back_to_the_current_directory() {
    let root = std::env::temp_dir().join(format!("er-ide-startup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("projeto");
    assert!(std::fs::create_dir_all(&project).is_ok());
    let current = PathBuf::from("/w/atual");

    let mut config = AppConfig::default();
    config.workspace.last_path = Some(project.clone());
    assert_eq!(
        startup_root(None, &config, Some(current.clone())),
        Some(project.clone()),
        "o último projeto tem prioridade sobre o diretório atual"
    );

    config.workspace.last_path = Some(root.join("removido"));
    assert_eq!(
        startup_root(None, &config, Some(current.clone())),
        Some(current.clone()),
        "uma pasta que sumiu não impede a IDE de abrir"
    );

    assert_eq!(
        startup_root(None, &AppConfig::default(), Some(current)),
        Some(PathBuf::from("/w/atual")),
        "sem registro, vale o diretório atual"
    );
    assert!(startup_root(None, &AppConfig::default(), None).is_none());
    let _ = std::fs::remove_dir_all(root);
}

fn snapshot_falho(
    provider: &str,
    nome: &str,
    extensoes: &[&str],
    estado: ide_language_api::ProviderState,
) -> ide_language_host::ProviderSnapshot {
    ide_language_host::ProviderSnapshot {
        metadata: ide_language_api::LanguageMetadata {
            language_id: LanguageId(provider.to_owned()),
            provider_id: ProviderId(provider.to_owned()),
            display_name: nome.to_owned(),
            extensions: extensoes.iter().map(|e| (*e).to_owned()).collect(),
            api_version: ide_language_api::LANGUAGE_API_VERSION,
            trigger_characters: Vec::new(),
        },
        capabilities: ide_language_api::LanguageCapabilities::empty(),
        state: estado,
        last_error: Some("faltou `npm install`".to_owned()),
    }
}

/// A queixa é da linguagem em uso, e não da primeira que estiver falha.
///
/// **Num projeto Java o provider de TypeScript está falho quase sempre**, e
/// deve estar: não há `node_modules` num projeto Java. Sem este filtro, uma
/// busca Java que não achasse nada responderia "TypeScript indisponível" — a
/// mensagem certa para o projeto errado, que é pior do que nenhuma.
#[test]
fn the_complaint_names_only_the_language_in_use() {
    let providers = vec![
        snapshot_falho(
            "typescript",
            "TypeScript",
            &["ts", "tsx"],
            ide_language_api::ProviderState::Failed,
        ),
        snapshot_falho(
            "java",
            "Java",
            &["java"],
            ide_language_api::ProviderState::Active,
        ),
    ];

    // Editando Java: o TypeScript falho não interessa a esta pergunta.
    assert_eq!(
        analisador_ausente(providers.clone(), &["java".to_owned()]),
        None,
        "uma busca Java não pode ser explicada por um analisador de TypeScript"
    );

    // Editando TypeScript: aí sim.
    let queixa = analisador_ausente(providers.clone(), &["ts".to_owned()]);
    assert!(
        queixa.is_some_and(|texto| texto.contains("TypeScript") && texto.contains("npm")),
        "a queixa precisa nomear a linguagem e dizer o que fazer"
    );

    // Sem arquivo aberto, calar é a resposta conservadora.
    assert_eq!(analisador_ausente(providers, &[]), None);
}

/// Desligado e caído não se dizem com a mesma palavra.
///
/// Dizer "indisponível" para quem desligou o provider faria procurar defeito
/// onde houve escolha — e é a mesma família de confusão que esta IDE já
/// enfrentou: a mensagem certa para a situação errada.
#[test]
fn a_disabled_analyzer_does_not_report_itself_as_broken() {
    let desligado = vec![snapshot_falho(
        "typescript",
        "TypeScript",
        &["ts"],
        ide_language_api::ProviderState::Disabled,
    )];
    let queixa = analisador_ausente(desligado, &["ts".to_owned()]);
    assert!(
        queixa
            .as_deref()
            .is_some_and(|texto| texto.contains("desligado")),
        "quem desligou precisa ler que desligou: {queixa:?}"
    );
    assert!(
        queixa.is_some_and(|texto| !texto.contains("indisponível")),
        "desligado não é indisponível"
    );
}

/// Uma linguagem sem analisador externo nenhum nunca produz queixa.
///
/// É o caso de qualquer linguagem que só tenha provider nativo — hoje o
/// realce de CSS, amanhã a próxima que entrar. Para elas, "nada encontrado"
/// é a resposta inteira, e inventar uma causa seria mentir.
#[test]
fn a_language_without_an_external_analyzer_never_complains() {
    let providers = vec![snapshot_falho(
        "css",
        "CSS",
        &["css", "scss"],
        ide_language_api::ProviderState::Active,
    )];
    assert_eq!(analisador_ausente(providers, &["css".to_owned()]), None);
}

#[test]
fn default_goals_compile_main_sources_in_each_build_system() {
    assert_eq!(
        default_goals(&maven_descriptor()),
        vec!["compile".to_owned()]
    );
    let gradle = ProjectDescriptor {
        build_system: BuildSystemId(GRADLE_BUILD_SYSTEM_ID.to_owned()),
        ..maven_descriptor()
    };
    assert_eq!(default_goals(&gradle), vec!["classes".to_owned()]);
}

/// Recolher uma escrita do Git pede o retrato — e é aí que ele é pedido.
///
/// Relatado: clicar em `Stage` e o arquivo não sair de "Alterados". A tela
/// pedia a escrita e o retrato ao mesmo tempo, e eram duas threads: o
/// `git status` costumava responder antes de o `git add` terminar, e a lista
/// mostrava o estado de **antes** da escrita. Só um refresh posterior — o
/// observador de disco, ou outra ação qualquer — corrigia a tela.
///
/// Agora a ordem é por construção: quando a resposta da escrita é recolhida, a
/// escrita terminou, e é então que se pergunta como o repositório ficou.
#[test]
fn recolher_uma_escrita_do_git_pede_o_retrato() {
    let mut ide = NativeIde::default();
    let raiz = std::env::temp_dir().join(format!("er-ide-escrita-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    assert!(std::fs::create_dir_all(&raiz).is_ok());
    ide.ui.shell = Some(test_shell(&raiz));

    // A resposta da escrita, já pronta: é ela que o laço recolhe.
    let (envio, receptor) = std::sync::mpsc::channel();
    let _ = envio.send("Git: stage".to_owned());
    let _ = ide.languages.git_write.start(receptor);
    assert!(
        ide.languages.git.pending.is_none(),
        "nada foi perguntado ainda"
    );

    assert!(ide.collect_git_write(), "a resposta da escrita foi recolhida");
    assert!(
        ide.languages.git.pending.is_some(),
        "e o retrato foi pedido depois dela, e não ao lado"
    );

    let _ = std::fs::remove_dir_all(&raiz);
}

/// Descartar apaga as marcas da margem e devolve o texto ao editor.
///
/// Descartar reescreve o arquivo com o que está no commit. Se a margem ficasse
/// como estava, ela marcaria de verde e vermelho uma alteração que já não
/// existe — e o editor continuaria mostrando o texto desfeito.
#[test]
fn descartar_apaga_as_marcas_e_devolve_o_texto() {
    let mut ide = NativeIde::default();
    let raiz = std::env::temp_dir().join(format!("er-ide-descarte-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    assert!(std::fs::create_dir_all(&raiz).is_ok());
    let arquivo = raiz.join("Pedido.java");
    assert!(std::fs::write(&arquivo, "class Pedido {}
").is_ok());
    ide.ui.shell = Some(test_shell(&raiz));

    // O arquivo aberto com o texto alterado, e a margem marcando a alteração.
    if let Some(shell) = ide.ui.shell.as_mut() {
        shell.show_document(&arquivo, "class Pedido { int novo; }
");
        shell.set_git_line_marks(arquivo.clone(), vec![(0, ide_ui::GitLineChange::Added)]);
        assert!(shell.git_marks_missing().is_none(), "a marca está lá");
    }

    // A escrita do descarte respondendo: o disco já voltou ao do commit.
    let (envio, receptor) = std::sync::mpsc::channel();
    let _ = envio.send("Pedido.java descartado".to_owned());
    let _ = ide.languages.git_write.start(receptor);
    ide.runtime.git_escreveu_em = Some(arquivo.clone());
    assert!(ide.collect_git_write());

    // O editor volta ao texto do disco…
    assert_eq!(
        ide.ui.shell.as_ref().and_then(ide_ui::IdeShell::active_text),
        Some("class Pedido {}
"),
        "o editor mostra o que ficou no disco"
    );
    // …e a margem foi perguntada de novo.
    assert!(
        !ide.languages.git_diff.is_empty(),
        "a margem daquele arquivo foi perguntada de novo"
    );

    // Quando a resposta chega — sem alteração nenhuma —, as marcas somem.
    let mut voltas = 0;
    while !ide.collect_git_diff() && voltas < 200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        voltas += 1;
    }
    // A margem é perguntada pela tela, e a tela só pergunta o que não sabe:
    // resposta vazia é resposta, e fica guardada como "este arquivo está igual
    // ao commit".
    assert!(
        ide.ui
            .shell
            .as_ref()
            .is_some_and(|shell| shell.git_marks_missing().is_none()),
        "a resposta chegou e ficou guardada"
    );
    assert!(
        ide.languages.git_diff.is_empty(),
        "e a fila esvaziou: não há pergunta pendente"
    );

    let _ = std::fs::remove_dir_all(&raiz);
}
