//! A tela de abertura: a marca enquanto o projeto carrega.
//!
//! # Por que ela existe
//!
//! A janela nasce **oculta** e só aparece no fim do arranque — depois de
//! registrar as linguagens, varrer o disco, detectar a ferramenta e importar o
//! projeto. Até lá não há nada na tela: quem abriu a IDE clica no ícone e
//! espera sem sinal nenhum de que alguma coisa está acontecendo.
//!
//! A tela de abertura ocupa esse intervalo. Ela não acelera nada; ela responde.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use ide_ui::splash_frame;
use ui_core::{ImageId, Size, Theme, WindowId};
use ui_render_api::{FrameInfo, PaintCommand, UiRenderer};
use ui_render_wgpu::WgpuRenderer;
use ui_window_api::WindowRequest;
use ui_window_winit::WinitWindow;
use winit::event_loop::ActiveEventLoop;

/// A marca, **embutida no binário**.
///
/// Embutida, e não lida do disco: a IDE instalada não tem a raiz do repositório
/// ao lado, e uma tela de abertura que depende de achar um arquivo é uma tela
/// de abertura que some na máquina de outra pessoa. O caminho é resolvido pelo
/// compilador, e um arquivo que sumir quebra a compilação em vez de o programa.
const MARCA: &[u8] = include_bytes!("../../../logoSplash-320.png");

/// O identificador da textura da marca.
///
/// Um só, e constante: a imagem é enviada uma vez para a placa e desenhada
/// enquanto a janela existir.
const MARCA_ID: ImageId = ImageId(1);

/// A marca decodificada, pronta para virar textura.
pub(super) struct Marca {
    pub(super) largura: u32,
    pub(super) altura: u32,
    pub(super) pixels: Vec<u8>,
}

/// Decodifica a marca uma vez, na primeira vez que alguém a pede.
///
/// `None` quando a imagem não pôde ser lida — e aí a IDE abre sem tela de
/// abertura, que é o comportamento de antes. Uma marca ilegível não pode
/// impedir ninguém de trabalhar.
pub(super) fn marca() -> Option<&'static Marca> {
    static DECODIFICADA: LazyLock<Option<Marca>> = LazyLock::new(decodificar);
    DECODIFICADA.as_ref()
}

fn decodificar() -> Option<Marca> {
    let decodificador = png::Decoder::new(std::io::Cursor::new(MARCA));
    let mut leitura = decodificador.read_info().ok()?;
    let mut bruto = vec![0; leitura.output_buffer_size()?];
    let info = leitura.next_frame(&mut bruto).ok()?;
    bruto.truncate(info.buffer_size());
    // A placa quer RGBA de oito bits. O que não estiver nesse formato é
    // convertido aqui, e o que não souber converter não vira tela de abertura.
    let pixels = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => bruto,
        (png::ColorType::Rgb, png::BitDepth::Eight) => bruto
            .chunks_exact(3)
            .flat_map(|cor| [cor[0], cor[1], cor[2], 0xFF])
            .collect(),
        _ => return None,
    };
    Some(Marca {
        largura: info.width,
        altura: info.height,
        pixels,
    })
}

/// A janela da tela de abertura, viva só durante o arranque.
///
/// Solta-la fecha a janela. É por isso que ela é um valor que quem abre a IDE
/// segura numa variável, e não um campo guardado em algum lugar: o tempo de
/// vida dela **é** o tempo do carregamento, e o compilador cobra isso.
pub(super) struct Splash {
    /// **Antes da janela, de propósito.** Os campos são soltos na ordem em que
    /// estão declarados, e a superfície de desenho foi criada a partir da
    /// janela: destruir a janela primeiro deixaria a superfície apontando para
    /// o que já não existe, e o sistema repinta a área não-cliente da janela
    /// enquanto ela morre — é a moldura aparecendo na hora de fechar.
    _renderer: WgpuRenderer,
    window: WinitWindow,
    aberta_em: Instant,
}

impl Drop for Splash {
    /// Sumir **antes** de morrer.
    ///
    /// Fechar uma janela é um processo com quadros no meio: o sistema pode
    /// repintar a moldura enquanto desmonta o que ela tinha. Tirá-la da tela
    /// primeiro faz esses quadros acontecerem onde ninguém os vê.
    fn drop(&mut self) {
        self.window.inner().set_visible(false);
    }
}

impl Splash {
    /// Quanto tempo a marca fica na tela, no mínimo.
    ///
    /// Não porque o arranque leve isso — muitas vezes leva menos —, mas porque
    /// é o tempo em que a IDE **usa a tela de abertura como abrigo**: o que sobe
    /// atrás dela é trabalho que aconteceria depois, com a janela já aberta e o
    /// usuário olhando um editor que ainda não responde.
    ///
    /// Terminar antes não encurta a espera; terminar depois a estende. O piso é
    /// só o piso.
    const MINIMO: Duration = Duration::from_secs(6);

    /// Se o tempo dela acabou.
    ///
    /// **Uma pergunta, e não uma espera.** Quem pergunta é o laço de eventos, a
    /// cada volta: bloquear até o tempo passar deixaria esta janela sem
    /// processar mensagens, e passados cinco segundos o sistema a declara
    /// travada e a substitui pela janela fantasma dele — com moldura, título e
    /// o aviso de "não respondendo". Era exatamente o que se via no fim.
    ///
    /// O trabalho que enche esse tempo não some: ele já está em threads
    /// próprias, e o laço continua girando para recolhê-lo.
    #[must_use]
    pub(super) fn terminou(&self) -> bool {
        tempo_esgotado(self.aberta_em, Self::MINIMO)
    }
}

/// Se um instante já ficou para trás o bastante.
///
/// Função livre porque é aqui que mora a **regra**, e a regra não precisa de
/// janela nem de placa de vídeo para ser conferida — a `Splash` precisa das
/// duas, e por isso nenhum teste conseguiria construí-la.
fn tempo_esgotado(inicio: Instant, minimo: Duration) -> bool {
    inicio.elapsed() >= minimo
}

/// Identidade da janela da tela de abertura.
///
/// Diferente da janela da IDE, que é a `1`: são duas janelas de verdade, e o
/// laço de eventos distingue uma da outra por aqui.
const JANELA: WindowId = WindowId(2);

/// Abre a tela de abertura: pequena, sem moldura e no meio da tela.
///
/// **Uma janela própria**, e não a janela da IDE mostrada mais cedo. A da IDE
/// abre maximizada, e uma marca de trezentos pontos no meio de uma tela inteira
/// não é uma tela de abertura — é uma janela quase vazia. A daqui tem o tamanho
/// da marca, e some quando a IDE aparece.
///
/// Devolve `None` quando qualquer parte falha, e aí a IDE abre como abria
/// antes: sem tela de abertura. Uma marca que não pôde ser desenhada não pode
/// impedir ninguém de trabalhar.
pub(super) fn abrir(event_loop: &ActiveEventLoop) -> Option<Splash> {
    let aberta_em = Instant::now();
    let marca = marca()?;
    let tamanho = Size::new(marca.largura as f32, marca.altura as f32);
    let window = WinitWindow::create_hidden(
        event_loop,
        JANELA,
        &WindowRequest {
            title: "ER IDE".to_owned(),
            logical_size: tamanho,
            maximized: false,
            // **Sem moldura desde o nascimento.** Barra de título, botões e
            // borda numa janela que vive segundos e não recebe gesto nenhum só
            // dizem que ela é uma janela — e tirá-los depois de criada faz o
            // sistema refazer o quadro: a barra aparecia por um instante na
            // hora de fechar, que é justamente quando se estava olhando.
            decorations: false,
            // **O único interruptor da transparência.** Escrito aqui, e lido
            // pelas outras camadas: a superfície pergunta à janela, e o
            // componente não pinta fundo nenhum. Assim só a marca aparece — sem
            // retângulo em volta dela.
            transparent: true,
        },
    )
    .ok()?;
    // Nada de mexer no estilo da janela depois de criada. Cada mudança de
    // estilo faz o sistema recalcular a moldura, e recalcular a moldura é
    // desenhá-la — mesmo numa janela que pediu para não ter nenhuma. Era daí
    // que vinha a barra de título piscando.
    centralizar(&window);

    // A superfície **pergunta à janela** se ela é transparente, em vez de
    // receber o dado de novo: um valor repetido é um valor que pode divergir.
    let mut renderer = pollster::block_on(WgpuRenderer::with_transparency(
        window.inner().clone(),
        window.is_transparent(),
    ))
    .ok()?;
    if let Err(error) =
        renderer.upload_rgba8(MARCA_ID, marca.largura, marca.altura, &marca.pixels)
    {
        tracing::warn!(%error, "a marca da tela de abertura não subiu para a placa");
        return None;
    }
    window.show();

    // Um quadro só, e apresentado na hora: o que vem depois desta chamada é
    // síncrono e não devolve o controle ao laço de eventos até terminar, então
    // não haveria um segundo quadro para desenhar. É por isso que a marca fica
    // parada em vez de girar — prometer movimento aqui seria mentir.
    let comandos = quadro(window.logical_size(), &Theme::dark());
    let desenhado = renderer
        .begin_frame(FrameInfo {
            window_id: window.handle().id,
            logical_size: window.logical_size(),
            scale_factor: window.scale_factor(),
        })
        .and_then(|()| renderer.submit(&comandos))
        .and_then(|()| renderer.end_frame());
    if let Err(error) = desenhado {
        tracing::warn!(%error, "a tela de abertura não pôde ser desenhada");
        return None;
    }
    Some(Splash {
        _renderer: renderer,
        window,
        aberta_em,
    })
}

/// Põe a janela no meio do monitor em que ela nasceu.
///
/// Em pixels de verdade, e não em pontos: a posição de uma janela é dada ao
/// sistema na unidade dele, e converter errado joga a tela de abertura para
/// fora do monitor num arranjo de duas telas com escalas diferentes.
fn centralizar(window: &WinitWindow) {
    let Some(monitor) = window.inner().current_monitor() else {
        return;
    };
    let area = monitor.size();
    let canto = monitor.position();
    let tamanho = window.inner().outer_size();
    window
        .inner()
        .set_outer_position(winit::dpi::PhysicalPosition::new(
            canto.x + (area.width as i32 - tamanho.width as i32) / 2,
            canto.y + (area.height as i32 - tamanho.height as i32) / 2,
        ));
}

/// O que desenhar na tela de abertura, para uma janela deste tamanho.
///
/// **Quem desenha é a biblioteca.** Aqui só se monta o componente e se pede a
/// pintura dele: a IDE traz a marca — que é dela, e só ela sabe qual é — e a
/// `SplashScreen` traz o desenho, com o fundo do tema e a marca no lugar. Foi
/// assim com o editor, com a árvore e com a lista; não seria diferente com a
/// tela que aparece antes de todas elas.
fn quadro(tamanho: Size, theme: &Theme) -> Vec<PaintCommand> {
    let Some(marca) = marca() else {
        return Vec::new();
    };
    splash_frame(
        MARCA_ID,
        Size::new(marca.largura as f32, marca.altura as f32),
        tamanho,
        theme,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A marca embutida decodifica, e vira RGBA de oito bits.
    ///
    /// É o teste que pega o arquivo trocado por um formato que a placa não
    /// aceita — a IDE continuaria abrindo, e a tela de abertura sumiria em
    /// silêncio.
    #[test]
    fn a_marca_embutida_decodifica_em_rgba() {
        let Some(marca) = marca() else {
            panic!("a marca embutida precisa decodificar");
        };
        assert!(marca.largura > 0 && marca.altura > 0);
        assert_eq!(
            marca.pixels.len(),
            marca.largura as usize * marca.altura as usize * 4,
            "a placa espera quatro bytes por pixel"
        );
    }

    /// O que a aplicação monta chega desenhado.
    ///
    /// **Onde** a marca fica e **o que** há atrás dela não se conferem aqui:
    /// isso é da `SplashScreen`, e está testado ao lado dela. O que este teste
    /// guarda é a costura — a marca decodificada vira um quadro com conteúdo.
    #[test]
    fn a_marca_decodificada_vira_um_quadro() {
        let comandos = quadro(Size::new(1280.0, 800.0), &Theme::dark());
        assert!(
            comandos
                .iter()
                .any(|comando| matches!(comando, PaintCommand::DrawImage(_))),
            "a marca precisa chegar ao quadro: {comandos:?}"
        );
    }

    /// O tempo é **perguntado**, e a pergunta responde na hora.
    ///
    /// Se ela voltar a esperar, esta janela deixa de processar mensagens e o
    /// sistema a troca pela janela fantasma dele. Por isso o teste mede quanto
    /// a pergunta demora: ela tem de ser imediata, mesmo quando a resposta é
    /// "ainda não".
    #[test]
    fn o_tempo_e_perguntado_e_a_pergunta_nao_espera() {
        let agora = Instant::now();
        assert!(
            !tempo_esgotado(agora, Duration::from_secs(30)),
            "recém-aberta, ela ainda não terminou"
        );
        assert!(
            tempo_esgotado(agora - Duration::from_secs(31), Duration::from_secs(30)),
            "passado o mínimo, terminou"
        );

        let marca = Instant::now();
        for _ in 0..1_000 {
            let _ = tempo_esgotado(agora, Duration::from_secs(30));
        }
        assert!(
            marca.elapsed() < Duration::from_millis(50),
            "perguntar não pode custar espera: mil perguntas levaram {:?}",
            marca.elapsed()
        );
    }

}
