//! Onde moram os tipos de uma dependência instalada.
//!
//! # Por que isto existe, contra o que a fase 1 decidiu
//!
//! A fase 1 da `25` deixou `node_modules` de fora **do índice**, e continua
//! certo: a busca por nome não pode encher de tipos que ninguém escreve. Mas ela
//! deixou de fora junto a **resolução** — `import { FormBuilder } from
//! '@angular/forms'` não levava a lugar nenhum —, e isso é outra coisa.
//!
//! São duas perguntas diferentes:
//!
//! | pergunta | `node_modules` entra? |
//! | --- | --- |
//! | quais tipos existem, para a busca | **não** |
//! | o que este tipo tem, depois do ponto | **sim** |
//!
//! Sem a segunda, a IDE só sabe o que o projeto declara — e ela é usada em
//! projetos diferentes, onde o que se injeta vem do framework. Medido na fase 8:
//! numa aplicação Angular, **24 dos 51** elos de cadeia que sobravam sem
//! resposta resolviam certo o nome do tipo e esbarravam em `@angular/forms`.
//!
//! # O que se lê, e nesta ordem
//!
//! É a ordem do Node e do TypeScript, e não uma nossa:
//!
//! 1. `exports`, que é o que os pacotes modernos usam e o que decide subcaminho
//!    — `@angular/forms/signals` é uma entrada própria;
//! 2. `types` ou `typings`, para o pacote inteiro;
//! 3. `index.d.ts` ao lado, que é o costume antigo;
//! 4. `@types/<pacote>`, para dependência escrita em JavaScript.

use std::path::{Path, PathBuf};

use super::arquivo_em;

/// O arquivo de tipos de um especificador nu, visto de um arquivo.
///
/// `None` quando o pacote não está instalado ou não declara tipo nenhum — e as
/// duas coisas acontecem: uma dependência só de JavaScript, sem `@types`, não
/// tem o que oferecer, e dizer que não se alcança é a resposta certa.
pub(super) fn resolver(de: &Path, especificador: &str) -> Option<PathBuf> {
    let (pacote, subcaminho) = partir(especificador)?;
    let mut atual = de.parent();
    while let Some(pasta) = atual {
        let modules = pasta.join("node_modules");
        if modules.is_dir() {
            if let Some(achado) = no_pacote(&modules.join(&pacote), subcaminho) {
                return Some(achado);
            }
            if let Some(achado) = no_pacote(&modules.join(tipos_a_parte(&pacote)), subcaminho) {
                return Some(achado);
            }
        }
        atual = pasta.parent();
    }
    None
}

/// O nome do pacote e o que vem depois dele.
///
/// `@angular/forms/signals` é o pacote `@angular/forms` e o subcaminho
/// `signals`: num nome com arroba, **os dois primeiros pedaços** são o pacote.
/// Cortar no primeiro procuraria um pacote chamado `@angular`, que não existe.
fn partir(especificador: &str) -> Option<(String, &str)> {
    if especificador.is_empty() || especificador.starts_with('.') {
        return None;
    }
    let mut pedacos = especificador.splitn(if especificador.starts_with('@') { 3 } else { 2 }, '/');
    let pacote = if especificador.starts_with('@') {
        let escopo = pedacos.next()?;
        let nome = pedacos.next()?;
        format!("{escopo}/{nome}")
    } else {
        pedacos.next()?.to_owned()
    };
    Some((pacote, pedacos.next().unwrap_or("")))
}

/// `@angular/forms` vira `@types/angular__forms`; `lodash` vira `@types/lodash`.
///
/// A troca da barra por dois sublinhados é a convenção do DefinitelyTyped, e não
/// uma escolha nossa — um `@types/@angular/forms` não existe em lugar nenhum.
fn tipos_a_parte(pacote: &str) -> String {
    format!("@types/{}", pacote.replacen('/', "__", 1).replacen('@', "", 1))
}

/// O arquivo de tipos dentro de uma pasta de pacote.
fn no_pacote(pasta: &Path, subcaminho: &str) -> Option<PathBuf> {
    if !pasta.is_dir() {
        return None;
    }
    let manifesto = pasta.join("package.json");
    if let Ok(texto) = std::fs::read_to_string(&manifesto)
        && let Ok(valor) = serde_json::from_str::<serde_json::Value>(&texto)
    {
        let chave = if subcaminho.is_empty() {
            ".".to_owned()
        } else {
            format!("./{subcaminho}")
        };
        if let Some(exports) = valor.get("exports")
            && let Some(caminho) = tipos_em_exports(exports, &chave)
            && let Some(achado) = arquivo_em(&pasta.join(caminho.trim_start_matches("./")))
        {
            return Some(achado);
        }
        if subcaminho.is_empty() {
            for campo in ["types", "typings"] {
                if let Some(escrito) = valor.get(campo).and_then(serde_json::Value::as_str)
                    && let Some(achado) = arquivo_em(&pasta.join(escrito.trim_start_matches("./")))
                {
                    return Some(achado);
                }
            }
        }
    }
    // Sem manifesto que ajude, o costume: o arquivo ou a pasta com `index`.
    let alvo = if subcaminho.is_empty() {
        pasta.to_path_buf()
    } else {
        pasta.join(subcaminho)
    };
    arquivo_em(&alvo)
}

/// O caminho de tipos de uma entrada do `exports`.
///
/// O valor de uma entrada pode ser um texto ou um mapa de **condições** —
/// `types`, `import`, `require`, `default` —, e as condições aninham. A que
/// interessa é `types`; um `default` que aponte para o JavaScript compilado não
/// serve, porque ali não há tipo nenhum para ler.
fn tipos_em_exports(exports: &serde_json::Value, chave: &str) -> Option<String> {
    let entrada = if exports.is_object() && exports.get(chave).is_some() {
        exports.get(chave)?
    } else if chave == "." && exports.is_string() {
        // `"exports": "./index.js"` é o pacote inteiro numa linha só.
        return None;
    } else {
        return None;
    };
    condicao_de_tipos(entrada)
}

fn condicao_de_tipos(valor: &serde_json::Value) -> Option<String> {
    if let Some(texto) = valor.as_str() {
        // Um texto direto só serve se já for uma declaração.
        return texto.ends_with(".d.ts").then(|| texto.to_owned());
    }
    let objeto = valor.as_object()?;
    if let Some(tipos) = objeto.get("types") {
        return condicao_de_tipos(tipos).or_else(|| tipos.as_str().map(str::to_owned));
    }
    // Condições de ambiente — `node`, `browser`, `default` — podem envolver a de
    // tipos. Descer nelas é o que faz um pacote que separa por ambiente
    // continuar achável.
    for (nome, dentro) in objeto {
        if nome.starts_with('.') {
            continue;
        }
        if let Some(achado) = condicao_de_tipos(dentro) {
            return Some(achado);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um `node_modules` de mentira, com as formas de declarar tipos.
    ///
    /// **O nome entra no caminho**, e isso não é enfeite: os testes rodam em
    /// paralelo no mesmo processo, e uma pasta compartilhada faz um teste apagar
    /// os arquivos do outro no meio da execução. A primeira versão daqui não
    /// tinha o nome, e dois testes falharam por isso — parecendo defeito do
    /// código que eles testavam.
    fn projeto(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("er-ts-dep-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let modules = raiz.join("node_modules");

        // Moderno: `exports` com condição de tipos, e um subcaminho próprio.
        escrever(
            &modules.join("@angular/forms/package.json"),
            "{\"exports\": {\".\": {\"types\": \"./types/forms.d.ts\", \"default\": \"./f.mjs\"},\
             \"./signals\": {\"types\": \"./types/signals.d.ts\"}}}",
        );
        escrever(
            &modules.join("@angular/forms/types/forms.d.ts"),
            "export declare class FormBuilder {\n  group(c: any): any;\n}\n",
        );
        escrever(
            &modules.join("@angular/forms/types/signals.d.ts"),
            "export declare class Signal {}\n",
        );

        // Antigo: `typings`.
        escrever(
            &modules.join("velho/package.json"),
            "{\"typings\": \"./lib/velho.d.ts\"}",
        );
        escrever(&modules.join("velho/lib/velho.d.ts"), "export declare class Velho {}\n");

        // Sem manifesto útil: `index.d.ts` ao lado.
        escrever(&modules.join("simples/package.json"), "{\"main\": \"./i.js\"}");
        escrever(&modules.join("simples/index.d.ts"), "export declare class Simples {}\n");

        // Escrito em JavaScript, com tipos à parte.
        escrever(&modules.join("sem-tipos/package.json"), "{\"main\": \"./i.js\"}");
        escrever(&modules.join("sem-tipos/i.js"), "module.exports = {};\n");
        escrever(
            &modules.join("@types/sem-tipos/index.d.ts"),
            "export declare class SemTipos {}\n",
        );

        escrever(&raiz.join("src/uso.ts"), "// nada\n");
        raiz
    }

    fn escrever(caminho: &Path, conteudo: &str) {
        if let Some(pasta) = caminho.parent() {
            assert!(std::fs::create_dir_all(pasta).is_ok());
        }
        assert!(std::fs::write(caminho, conteudo).is_ok());
    }

    /// O nome do pacote são os dois primeiros pedaços quando há arroba.
    #[test]
    fn a_scoped_package_keeps_both_of_its_parts() {
        assert_eq!(
            partir("@angular/forms/signals"),
            Some(("@angular/forms".to_owned(), "signals"))
        );
        assert_eq!(partir("@angular/forms"), Some(("@angular/forms".to_owned(), "")));
        assert_eq!(partir("rxjs/operators"), Some(("rxjs".to_owned(), "operators")));
        assert_eq!(partir("rxjs"), Some(("rxjs".to_owned(), "")));
    }

    /// A convenção do DefinitelyTyped para pacote com escopo.
    #[test]
    fn scoped_packages_have_a_mangled_types_package() {
        assert_eq!(tipos_a_parte("@angular/forms"), "@types/angular__forms");
        assert_eq!(tipos_a_parte("lodash"), "@types/lodash");
    }

    /// **As quatro formas de um pacote declarar seus tipos.**
    #[test]
    fn the_four_ways_a_package_declares_its_types() {
        let raiz = projeto("formas");
        let de = raiz.join("src/uso.ts");
        let achar = |especificador: &str| -> Option<String> {
            resolver(&de, especificador).and_then(|caminho| {
                caminho
                    .file_name()
                    .and_then(|nome| nome.to_str())
                    .map(str::to_owned)
            })
        };
        assert_eq!(achar("@angular/forms").as_deref(), Some("forms.d.ts"));
        assert_eq!(achar("@angular/forms/signals").as_deref(), Some("signals.d.ts"));
        assert_eq!(achar("velho").as_deref(), Some("velho.d.ts"));
        assert_eq!(achar("simples").as_deref(), Some("index.d.ts"));
        assert_eq!(
            achar("sem-tipos").as_deref(),
            Some("index.d.ts"),
            "o pacote é JavaScript, e os tipos estão em @types"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Um pacote que não existe continua sem resposta.
    ///
    /// **"Não alcanço" é uma resposta**, e apontar para um arquivo que não
    /// existe seria pior do que ela.
    #[test]
    fn a_package_that_is_not_installed_has_no_answer() {
        let raiz = projeto("ausente");
        assert!(resolver(&raiz.join("src/uso.ts"), "nao-instalado").is_none());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// O `node_modules` é procurado **subindo**, como o Node faz.
    ///
    /// Num monorepo, a dependência está na raiz do repositório e o código está
    /// alguns níveis abaixo. Procurar só ao lado acharia quase nada.
    #[test]
    fn node_modules_is_searched_upwards() {
        let raiz = projeto("subindo");
        let fundo = raiz.join("src/a/b/c/uso.ts");
        escrever(&fundo, "// nada\n");
        assert!(resolver(&fundo, "@angular/forms").is_some());
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
