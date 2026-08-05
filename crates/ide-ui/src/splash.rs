//! A tela de abertura, composta.
//!
//! A IDE **não desenha**: ela escolhe o componente e diz onde ele fica. Quem
//! pinta o fundo, centra a marca e decide como isso muda com o tamanho da
//! janela é a `SplashScreen` da biblioteca.
//!
//! Este módulo existe porque a aplicação — que é quem tem a janela, a placa e a
//! imagem — não conhece componente nenhum, e não deve conhecer. Ela pede os
//! comandos aqui e os entrega ao renderizador.

use ui_api::{LayoutContext, PaintContext, Widget};
use ui_components::SplashScreen;
use ui_core::{ImageId, Rect, Size, Theme, WidgetId};
use ui_render_api::PaintCommand;

/// Identidade do componente da tela de abertura.
///
/// Ele vive sozinho numa janela própria, e some antes de a IDE existir: não
/// disputa identidade com nada.
const SPLASH: WidgetId = WidgetId(1);

/// O que desenhar na tela de abertura de um dado tamanho.
///
/// A marca já está na placa; o que chega aqui é o identificador dela e o
/// tamanho em que foi enviada.
#[must_use]
pub fn splash_frame(
    image: ImageId,
    image_size: Size,
    area: Size,
    theme: &Theme,
) -> Vec<PaintCommand> {
    let mut tela = SplashScreen::new(SPLASH, image, image_size);
    tela.layout(
        &LayoutContext::default(),
        Rect::new(0.0, 0.0, area.width, area.height),
    );
    let mut pintura = PaintContext::with_theme(*theme);
    tela.paint(&mut pintura);
    pintura.into_commands()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui_render_api::PaintCommand;

    /// **Só a marca**, centrada, e nada atrás dela.
    ///
    /// Um fundo aqui apagaria a transparência que a janela pediu — e seria um
    /// terceiro lugar tendo de concordar com os outros dois.
    #[test]
    fn so_a_marca_e_ela_fica_no_meio() {
        let area = Size::new(320.0, 320.0);
        let comandos = splash_frame(ImageId(7), Size::new(200.0, 100.0), area, &Theme::dark());

        let [PaintCommand::DrawImage(imagem)] = comandos.as_slice() else {
            panic!("a tela de abertura é a marca, e só ela: {comandos:?}");
        };
        assert_eq!(imagem.image_id, ImageId(7));
        let centro_x = imagem.destination.origin.x + imagem.destination.size.width / 2.0;
        let centro_y = imagem.destination.origin.y + imagem.destination.size.height / 2.0;
        assert!(
            (centro_x - area.width / 2.0).abs() < 0.01
                && (centro_y - area.height / 2.0).abs() < 0.01,
            "a marca fica no centro: {:?}",
            imagem.destination
        );
    }

    /// A marca sai no tamanho em que foi enviada, e não esticada.
    #[test]
    fn a_marca_nao_estica_para_preencher() {
        let tamanho = Size::new(120.0, 60.0);
        let comandos = splash_frame(ImageId(1), tamanho, Size::new(800.0, 600.0), &Theme::dark());
        let [PaintCommand::DrawImage(imagem)] = comandos.as_slice() else {
            panic!("a marca precisa ser desenhada");
        };
        assert_eq!(imagem.destination.size, tamanho);
    }
}
