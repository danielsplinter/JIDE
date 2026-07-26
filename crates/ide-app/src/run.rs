//! Execução da aplicação do projeto, com ou sem depuração.
//!
//! A IDE só afirma um comando quando consegue deduzi-lo com segurança a partir
//! do projeto importado. Onde não consegue, prefere dizer que não sabe a
//! inventar um comando que falharia — e o usuário resolve com uma linha de
//! configuração.

/// Argumento que liga o agente de depuração da JVM na porta escolhida.
pub(crate) fn agent_argument(host: &str, port: u16) -> String {
    format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address={host}:{port}")
}

/// Projeto que a IDE sabe subir, descrito sem depender do modelo completo.
pub(crate) struct RunTarget<'a> {
    pub(crate) build_system: &'a str,
    /// Wrapper versionado no projeto, quando existir.
    pub(crate) wrapper: Option<String>,
    pub(crate) spring_boot: bool,
}

/// Execução comum ou com o depurador conectado.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunMode<'a> {
    Plain,
    Debug { host: &'a str, port: u16 },
}

impl RunMode<'_> {
    /// Trecho que liga a depuração, vazio na execução comum.
    fn agent(self) -> String {
        match self {
            Self::Plain => String::new(),
            Self::Debug { host, port } => agent_argument(host, port),
        }
    }
}

/// Comando que sobe a aplicação do projeto.
pub(crate) fn run_command(
    configured: Option<&str>,
    target: Option<&RunTarget<'_>>,
    mode: RunMode<'_>,
) -> Option<String> {
    if let Some(configured) = configured.map(str::trim).filter(|value| !value.is_empty()) {
        let (host, port) = match mode {
            RunMode::Plain => (String::new(), String::new()),
            RunMode::Debug { host, port } => (host.to_owned(), port.to_string()),
        };
        return Some(
            configured
                .replace("{agent}", &mode.agent())
                .replace("{host}", &host)
                .replace("{port}", &port)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    let target = target?;
    match target.build_system {
        "maven" if target.spring_boot => {
            let executable = target.wrapper.clone().unwrap_or_else(|| "mvn".to_owned());
            // `spring-boot:run` encadeia a fase `test-compile`, então um teste
            // que não compila impediria de executar a aplicação. Executar não é
            // testar: as fontes de teste ficam de fora.
            //
            // Todo argumento `-D` vai entre aspas: o PowerShell parte o token
            // no primeiro ponto e o Maven receberia uma fase inexistente.
            Some(match mode {
                RunMode::Plain => {
                    format!("{executable} -B \"-Dmaven.test.skip=true\" spring-boot:run")
                }
                RunMode::Debug { .. } => format!(
                    "{executable} -B \"-Dmaven.test.skip=true\" spring-boot:run \"-Dspring-boot.run.jvmArguments={}\"",
                    mode.agent()
                ),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spring_maven() -> RunTarget<'static> {
        RunTarget {
            build_system: "maven",
            wrapper: None,
            spring_boot: true,
        }
    }

    #[test]
    fn spring_boot_maven_runs_with_and_without_the_debug_agent() {
        let plain = run_command(None, Some(&spring_maven()), RunMode::Plain);
        assert_eq!(
            plain.as_deref(),
            Some("mvn -B \"-Dmaven.test.skip=true\" spring-boot:run")
        );

        let debugging = run_command(
            None,
            Some(&spring_maven()),
            RunMode::Debug {
                host: "127.0.0.1",
                port: 8000,
            },
        );
        assert_eq!(
            debugging.as_deref(),
            Some(
                "mvn -B \"-Dmaven.test.skip=true\" spring-boot:run \"-Dspring-boot.run.jvmArguments=-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=127.0.0.1:8000\""
            )
        );
    }

    #[test]
    fn running_the_application_never_compiles_the_test_sources() {
        // `spring-boot:run` encadeia `test-compile`; um teste quebrado não pode
        // impedir de executar a aplicação.
        for mode in [
            RunMode::Plain,
            RunMode::Debug {
                host: "127.0.0.1",
                port: 8000,
            },
        ] {
            assert!(
                run_command(None, Some(&spring_maven()), mode)
                    .is_some_and(|command| command.contains("\"-Dmaven.test.skip=true\"")),
                "o argumento precisa das aspas: o PowerShell parte o token no ponto"
            );
        }
    }

    #[test]
    fn the_wrapper_is_preferred_when_the_project_has_one() {
        let target = RunTarget {
            wrapper: Some("./mvnw".to_owned()),
            ..spring_maven()
        };
        assert!(
            run_command(None, Some(&target), RunMode::Plain)
                .is_some_and(|command| command.starts_with("./mvnw -B"))
        );
    }

    #[test]
    fn the_configured_command_wins_and_the_agent_marker_disappears_without_debug() {
        let configured = Some("./gradlew bootRun {agent}");
        assert_eq!(
            run_command(configured, Some(&spring_maven()), RunMode::Plain).as_deref(),
            Some("./gradlew bootRun"),
            "sem depuração o marcador do agente sai e não deixa espaço sobrando"
        );
        assert_eq!(
            run_command(
                configured,
                None,
                RunMode::Debug {
                    host: "10.0.0.5",
                    port: 9000
                }
            )
            .as_deref(),
            Some(
                "./gradlew bootRun -agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=10.0.0.5:9000"
            )
        );
        assert!(run_command(Some("   "), None, RunMode::Plain).is_none());
    }

    #[test]
    fn placeholders_for_host_and_port_are_expanded_only_when_debugging() {
        let configured = Some("run --debug-port {port} --host {host}");
        assert_eq!(
            run_command(
                configured,
                None,
                RunMode::Debug {
                    host: "127.0.0.1",
                    port: 8000
                }
            )
            .as_deref(),
            Some("run --debug-port 8000 --host 127.0.0.1")
        );
        assert_eq!(
            run_command(configured, None, RunMode::Plain).as_deref(),
            Some("run --debug-port --host"),
            "sem depuração não existe porta a informar"
        );
    }

    #[test]
    fn projects_the_ide_cannot_start_report_no_command() {
        for target in [
            RunTarget {
                spring_boot: false,
                ..spring_maven()
            },
            RunTarget {
                build_system: "gradle",
                wrapper: Some("./gradlew".to_owned()),
                spring_boot: true,
            },
        ] {
            assert!(run_command(None, Some(&target), RunMode::Plain).is_none());
            assert!(
                run_command(
                    None,
                    Some(&target),
                    RunMode::Debug {
                        host: "127.0.0.1",
                        port: 8000
                    }
                )
                .is_none()
            );
        }
        assert!(run_command(None, None, RunMode::Plain).is_none());
    }
}
