//! O medidor, contra processos de verdade.
//!
//! Medir memória é o tipo de coisa que "compila e parece certo" enquanto devolve
//! zero para sempre. Os testes aqui cobram número, e não ausência de erro.

use ide_core::{MemoryMeter, MemoryReading};
use ide_domain::ProcessId;

/// O processo da IDE sempre custa alguma coisa.
///
/// Zero aqui significaria que a consulta falhou em silêncio — que é exatamente
/// o modo como um medidor quebrado se parece com um medidor bom.
#[test]
fn the_own_process_always_reports_something() {
    let leitura = MemoryMeter::read(&[]);
    assert!(
        leitura.own_mb > 0,
        "o próprio processo precisa aparecer na medição: {leitura:?}"
    );
    assert_eq!(
        leitura.external_mb, 0,
        "sem processo externo informado, não há externo a contar"
    );
}

/// Um processo externo entra na conta, e separado do nosso.
#[test]
fn an_external_process_is_counted_apart() {
    #[cfg(windows)]
    let filho = std::process::Command::new("cmd").args(["/C", "pause"]).spawn();
    #[cfg(not(windows))]
    let filho = std::process::Command::new("sleep").arg("30").spawn();
    // Sem shell disponível não há o que medir; o resto dos testes cobre o
    // caminho principal.
    let Ok(mut filho) = filho else {
        return;
    };

    let id = ProcessId(u64::from(filho.id()));
    let leitura = MemoryMeter::read(&[id]);
    assert!(
        leitura.external_mb > 0,
        "o processo externo vivo precisa somar: {leitura:?}"
    );
    assert_eq!(leitura.total_mb(), leitura.own_mb + leitura.external_mb);

    let _ = filho.kill();
    let _ = filho.wait();
}

/// Um processo que já morreu não inventa memória.
///
/// É a razão de o medidor não guardar a lista: quem a guardasse continuaria
/// contando mortos. Recebendo-a de fora, um PID vencido simplesmente não é
/// encontrado, e some da soma sozinho.
#[test]
fn a_dead_process_adds_nothing() {
    #[cfg(windows)]
    let filho = std::process::Command::new("cmd").args(["/C", "exit"]).spawn();
    #[cfg(not(windows))]
    let filho = std::process::Command::new("true").spawn();
    let Ok(mut filho) = filho else {
        return;
    };
    let id = ProcessId(u64::from(filho.id()));
    let _ = filho.wait();

    let leitura = MemoryMeter::read(&[id]);
    assert_eq!(
        leitura.external_mb, 0,
        "um PID morto não pode somar nada: {leitura:?}"
    );
}

/// O total é a soma, e é o número que importa para a máquina.
#[test]
fn the_total_is_the_sum() {
    let leitura = MemoryReading {
        own_mb: 300,
        external_mb: 206,
    };
    assert_eq!(leitura.total_mb(), 506);
}
