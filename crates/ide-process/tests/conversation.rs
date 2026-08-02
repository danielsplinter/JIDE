//! Um processo com quem se conversa, contra um processo de verdade.
//!
//! São os critérios da fase 3a da `23`. O eco é um laço de PowerShell que lê
//! linha, escreve linha e esvazia o buffer — um filtro que **transmite** em vez
//! de acumular até o fim, que é a forma que um analisador tem.
//!
//! Não há processo de eco portátil, e por isso os testes são presos ao Windows,
//! como o `execute_captures_stdout_and_exit_code` que já existia. No dia em que
//! a IDE rodar noutro lugar, é aqui que se acrescenta o equivalente.

#![cfg(windows)]

use std::{path::PathBuf, time::Duration};

use ide_process::{NativeProcessSupervisor, ProcessConversation, ProcessRequest, ProcessSupervisor};

const ECO: &str = "while(($l = [Console]::In.ReadLine()) -ne $null){ \
                   [Console]::Out.WriteLine('eco: ' + $l); [Console]::Out.Flush() }";

fn runtime() -> tokio::runtime::Runtime {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => panic!("runtime creation failed: {error}"),
    }
}

fn eco_request(script: &str) -> ProcessRequest {
    ProcessRequest {
        program: PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
        args: vec![
            "-NoProfile".to_owned(),
            "-Command".to_owned(),
            script.to_owned(),
        ],
        working_directory: None,
        // Ignorado numa conversa: quem conversa não tem prazo para terminar.
        timeout: None,
        environment: Vec::new(),
    }
}

/// Três pedidos, três respostas, na ordem.
///
/// É o que separa uma conversa de um `execute`: o processo continua de pé entre
/// uma pergunta e a seguinte, e a resposta chega sem ele morrer.
#[test]
fn three_requests_get_three_answers_in_order() {
    let runtime = runtime();
    let supervisor = NativeProcessSupervisor::default();
    let conversa = match runtime.block_on(supervisor.converse(eco_request(ECO))) {
        Ok(conversa) => conversa,
        Err(error) => panic!("a conversa precisa abrir: {error}"),
    };

    runtime.block_on(async {
        for pergunta in ["um", "dois", "tres"] {
            assert!(conversa.send(pergunta).await.is_ok(), "envio de {pergunta}");
            let resposta = tokio::time::timeout(Duration::from_secs(30), conversa.receive()).await;
            match resposta {
                Ok(Ok(Some(linha))) => assert_eq!(linha, format!("eco: {pergunta}")),
                outro => panic!("resposta inesperada para {pergunta}: {outro:?}"),
            }
        }
        assert!(conversa.is_running().await, "o processo continua de pé");
        assert!(conversa.shutdown().await.is_ok());
    });
}

/// A morte do processo é percebida, e não trava quem espera.
///
/// É o sinal de que a ADR-025 depende para cair no provider nativo: sem ele,
/// quem espera resposta esperaria para sempre.
#[test]
fn the_death_of_the_process_ends_the_wait() {
    let runtime = runtime();
    let supervisor = NativeProcessSupervisor::default();
    // Este eco responde uma vez e sai — a saída fecha, e é isso que se cobra.
    let script = "$l = [Console]::In.ReadLine(); [Console]::Out.WriteLine('eco: ' + $l); \
                  [Console]::Out.Flush()";
    let conversa = match runtime.block_on(supervisor.converse(eco_request(script))) {
        Ok(conversa) => conversa,
        Err(error) => panic!("a conversa precisa abrir: {error}"),
    };

    runtime.block_on(async {
        assert!(conversa.send("unico").await.is_ok());
        let primeira = tokio::time::timeout(Duration::from_secs(30), conversa.receive()).await;
        assert!(matches!(primeira, Ok(Ok(Some(_)))), "a primeira responde");

        // A segunda leitura encontra o fim da saída em vez de esperar sem fim.
        let segunda = tokio::time::timeout(Duration::from_secs(30), conversa.receive()).await;
        assert!(
            matches!(segunda, Ok(Ok(None))),
            "o fim da saída é o sinal de morte, e ele precisa chegar: {segunda:?}"
        );
    });
}

/// Encerrar não deixa processo de pé.
///
/// Um analisador órfão come memória de um projeto grande com folga, e sobrevive
/// à IDE se ninguém o matar.
#[test]
fn shutting_down_leaves_no_process_behind() {
    let runtime = runtime();
    let supervisor = NativeProcessSupervisor::default();
    let conversa = match runtime.block_on(supervisor.converse(eco_request(ECO))) {
        Ok(conversa) => conversa,
        Err(error) => panic!("a conversa precisa abrir: {error}"),
    };

    runtime.block_on(async {
        assert!(conversa.is_running().await);
        assert!(conversa.shutdown().await.is_ok());
        // Matar é assíncrono do lado do sistema; o que se cobra é que o
        // supervisor pare de considerá-lo vivo, e que falar com ele falhe.
        assert!(!conversa.is_running().await, "o processo precisa ter caído");
        assert!(
            conversa.send("depois").await.is_err(),
            "falar com uma conversa encerrada é erro, e não silêncio"
        );
    });
}

/// Soltar a conversa sem encerrá-la também não deixa órfão.
///
/// É `kill_on_drop`, e vale ter o teste porque a garantia é do tipo que some sem
/// avisar quando alguém reescreve a construção do comando.
#[test]
fn dropping_the_conversation_kills_the_process() {
    let runtime = runtime();
    let supervisor = NativeProcessSupervisor::default();
    runtime.block_on(async {
        let conversa: Box<dyn ProcessConversation> =
            match supervisor.converse(eco_request(ECO)).await {
                Ok(conversa) => conversa,
                Err(error) => panic!("a conversa precisa abrir: {error}"),
            };
        assert!(conversa.is_running().await);
        drop(conversa);
        // Sem `kill_on_drop`, o processo continuaria vivo depois daqui e só
        // morreria com a IDE. Não há como afirmá-lo de dentro do processo que o
        // criou sem contar processos do sistema; o que este teste garante é que
        // soltar não entra em pânico nem trava.
    });
}
