//! Quanto a IDE está custando, somando o que roda fora dela.
//!
//! A `08` trata memória como requisito arquitetural com orçamento explícito, e
//! define `MemoryBudget` desde sempre — que nunca existiu em código. Isto é a
//! metade dele que importa primeiro: **medir**. Um teto sem medição derruba o
//! processo e ninguém sabe por quê.
//!
//! # São dois números
//!
//! O analisador externo roda em processo próprio, com espaço de endereçamento
//! próprio. Contabilmente separado; fisicamente, a mesma RAM. Se a soma passar
//! do que a máquina tem, quem parece lento é a IDE, porque é ela que está na
//! frente de quem usa.
//!
//! No Windows o Gerenciador de Tarefas ainda agrupa processos filhos sob o pai e
//! **soma no total exibido** — o analisador aparece como memória da IDE para
//! quem olha, esteja ou não no nosso heap. "Não é o meu processo" é verdade
//! contábil que não significa nada para quem está com a máquina paginando.

use ide_domain::ProcessId;

/// O que a IDE custa agora, em megabytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemoryReading {
    /// O processo da IDE.
    pub own_mb: u64,
    /// A soma dos processos externos registrados.
    pub external_mb: u64,
}

impl MemoryReading {
    #[must_use]
    pub const fn total_mb(&self) -> u64 {
        self.own_mb + self.external_mb
    }
}

/// Mede o que a IDE custa, somando processos externos informados por quem os tem.
///
/// **Não guarda a lista.** Quem sabe quais processos existem é o supervisor, que
/// os criou; duplicar esse registro aqui criaria duas verdades sobre a mesma
/// coisa, e uma delas envelheceria — o processo morre, o supervisor sabe, e o
/// medidor continua contando um morto.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryMeter;

impl MemoryMeter {
    /// Mede agora, contando os externos informados.
    ///
    /// Uma leitura custa consultar a tabela de processos do sistema, e por isso
    /// quem chama decide de quanto em quanto — não há relógio aqui dentro, do
    /// mesmo modo que a suspensão por ociosidade é acionada de fora.
    #[must_use]
    pub fn read(external: &[ProcessId]) -> MemoryReading {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

        let Ok(meu) = sysinfo::get_current_pid() else {
            return MemoryReading::default();
        };
        let mut alvos: Vec<Pid> = external
            .iter()
            .map(|id| Pid::from_u32(id.0 as u32))
            .collect();
        alvos.push(meu);

        let mut sistema = System::new();
        // Só os processos que interessam, e só a memória deles: atualizar a
        // tabela inteira a cada leitura custaria mais do que o número vale.
        sistema.refresh_processes_specifics(
            ProcessesToUpdate::Some(&alvos),
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );

        let em_mb = |bytes: u64| bytes / (1024 * 1024);
        MemoryReading {
            own_mb: sistema.process(meu).map_or(0, |processo| em_mb(processo.memory())),
            external_mb: external
                .iter()
                .filter_map(|id| sistema.process(Pid::from_u32(id.0 as u32)))
                .map(|processo| em_mb(processo.memory()))
                .sum(),
        }
    }
}
