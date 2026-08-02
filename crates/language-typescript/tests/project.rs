//! O projeto lido de onde ele está escrito.
//!
//! O que se cobra aqui é a ADR-027: as raízes saem do `tsconfig.json`, e não de
//! convenção. Um palpite de `src` passaria em todos estes testes com um projeto
//! comum — e falharia em silêncio no primeiro que não seguisse a convenção, que
//! é exatamente o defeito que a decisão evita.

use std::path::{Path, PathBuf};

use ide_project::build::{BuildSystemAdapter, ProjectImportRequest};
use ide_process::NativeProcessSupervisor;
use language_typescript::{NpmAdapter, scripts, tsconfig};

fn temporary(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("er-ts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    root
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        assert!(std::fs::create_dir_all(parent).is_ok());
    }
    assert!(std::fs::write(path, contents).is_ok());
}

/// O arquivo padrão da CLI do TypeScript vem cheio de comentários.
///
/// Um leitor de JSON estrito recusaria o arquivo que a própria ferramenta gera —
/// e recusar viraria "não sei quais são as raízes" no projeto mais comum que
/// existe.
#[test]
fn the_config_is_read_with_comments_and_trailing_commas() {
    let root = temporary("comentarios");
    write(
        &root.join("tsconfig.json"),
        r#"{
  // O que entra na compilação
  "compilerOptions": {
    /* a saída não é código-fonte */
    "outDir": "build",
  },
  "include": ["app/**/*.ts"],
}"#,
    );

    let Ok(config) = tsconfig::load(&root.join("tsconfig.json")) else {
        panic!("um tsconfig com comentários precisa ser lido");
    };
    assert_eq!(config.source_roots(), vec![root.join("app")]);
    assert_eq!(config.out_dir, Some(PathBuf::from("build")));
    let _ = std::fs::remove_dir_all(&root);
}

/// Uma barra dentro de texto não é comentário.
#[test]
fn a_slash_inside_a_string_is_not_a_comment() {
    let root = temporary("barra");
    write(
        &root.join("tsconfig.json"),
        r#"{ "include": ["src//gerado/**/*"] }"#,
    );
    let Ok(config) = tsconfig::load(&root.join("tsconfig.json")) else {
        panic!("o texto com barras precisa sobreviver");
    };
    assert_eq!(config.include, vec!["src//gerado/**/*".to_owned()]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `extends` resolve caminhos relativos ao arquivo onde eles foram escritos.
///
/// É a regra do TypeScript, e ignorá-la quebraria o caso mais comum de
/// monorepo: um `tsconfig.base.json` uma pasta acima.
#[test]
fn what_the_config_extends_becomes_the_base() {
    let root = temporary("extends");
    write(
        &root.join("tsconfig.base.json"),
        r#"{ "compilerOptions": { "outDir": "dist" }, "include": ["base/**/*"] }"#,
    );
    write(
        &root.join("app").join("tsconfig.json"),
        r#"{ "extends": "../tsconfig.base.json" }"#,
    );

    let Ok(config) = tsconfig::load(&root.join("app").join("tsconfig.json")) else {
        panic!("o tsconfig que estende precisa ser lido");
    };
    // O `include` veio da base, mas vale a partir de quem estende.
    assert_eq!(config.source_roots(), vec![root.join("app").join("base")]);
    assert_eq!(config.out_dir, Some(PathBuf::from("dist")));
    let _ = std::fs::remove_dir_all(&root);
}

/// O filho substitui o `include` da base, e não soma a ele.
///
/// Somar faria um projeto herdar pastas que ele declarou não querer.
#[test]
fn the_child_replaces_what_it_declares() {
    let root = temporary("substitui");
    write(
        &root.join("tsconfig.base.json"),
        r#"{ "include": ["base/**/*"], "exclude": ["base/testes"] }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{ "extends": "./tsconfig.base.json", "include": ["proprio/**/*"] }"#,
    );

    let Ok(config) = tsconfig::load(&root.join("tsconfig.json")) else {
        panic!("o tsconfig precisa ser lido");
    };
    assert_eq!(config.source_roots(), vec![root.join("proprio")]);
    // O que não foi redeclarado continua vindo da base.
    assert_eq!(config.exclude, vec!["base/testes".to_owned()]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `rootDir` é explícito e vence o palpite do `include`.
#[test]
fn an_explicit_root_wins() {
    let root = temporary("rootdir");
    write(
        &root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "rootDir": "codigo" }, "include": ["outra/**/*"] }"#,
    );
    let Ok(config) = tsconfig::load(&root.join("tsconfig.json")) else {
        panic!("o tsconfig precisa ser lido");
    };
    assert_eq!(config.source_roots(), vec![root.join("codigo")]);
    let _ = std::fs::remove_dir_all(&root);
}

/// Uma raiz contida em outra é ruído, e o mesmo arquivo apareceria duas vezes.
#[test]
fn a_root_inside_another_is_dropped() {
    let root = temporary("aninhada");
    write(
        &root.join("tsconfig.json"),
        r#"{ "include": ["src/**/*", "src/app/**/*", "testes/**/*"] }"#,
    );
    let Ok(config) = tsconfig::load(&root.join("tsconfig.json")) else {
        panic!("o tsconfig precisa ser lido");
    };
    assert_eq!(
        config.source_roots(),
        vec![root.join("src"), root.join("testes")]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Sem `include` nem `files`, o projeto é o diretório do arquivo.
///
/// É o padrão do compilador. Chutar `src` aqui seria a convenção que a ADR-027
/// recusa — e estaria errada em todo projeto que não a seguisse.
#[test]
fn without_include_the_project_is_the_directory() {
    let root = temporary("sem-include");
    write(&root.join("tsconfig.json"), r#"{ "compilerOptions": {} }"#);
    let Ok(config) = tsconfig::load(&root.join("tsconfig.json")) else {
        panic!("o tsconfig precisa ser lido");
    };
    assert_eq!(config.source_roots(), vec![root.clone()]);
    // Sem `exclude` declarado valem os padrões do próprio TypeScript.
    assert!(config.excluded().contains(&root.join("node_modules")));
    let _ = std::fs::remove_dir_all(&root);
}

/// Dois projetos no mesmo diretório não misturam os conjuntos.
///
/// É o critério da fase 2 da `23`.
#[test]
fn two_projects_do_not_mix_their_roots() {
    let root = temporary("dois");
    write(
        &root.join("um").join("tsconfig.json"),
        r#"{ "include": ["fonte/**/*"] }"#,
    );
    write(
        &root.join("outro").join("tsconfig.json"),
        r#"{ "include": ["codigo/**/*"] }"#,
    );

    let Ok(um) = tsconfig::load(&root.join("um").join("tsconfig.json")) else {
        panic!("o primeiro precisa ser lido");
    };
    let Ok(outro) = tsconfig::load(&root.join("outro").join("tsconfig.json")) else {
        panic!("o segundo precisa ser lido");
    };
    assert_eq!(um.source_roots(), vec![root.join("um").join("fonte")]);
    assert_eq!(outro.source_roots(), vec![root.join("outro").join("codigo")]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `extends` em círculo não trava a abertura do projeto.
#[test]
fn a_cycle_of_extends_does_not_hang() {
    let root = temporary("ciclo");
    write(
        &root.join("a.json"),
        r#"{ "extends": "./b.json", "include": ["a/**/*"] }"#,
    );
    write(&root.join("b.json"), r#"{ "extends": "./a.json" }"#);
    let Ok(config) = tsconfig::load(&root.join("a.json")) else {
        panic!("um ciclo precisa parar, e não travar");
    };
    assert_eq!(config.source_roots(), vec![root.join("a")]);
    let _ = std::fs::remove_dir_all(&root);
}

/// O projeto é reconhecido pelo `package.json`, e as raízes vêm do `tsconfig`.
#[test]
fn the_model_takes_its_roots_from_the_config() {
    let root = temporary("modelo");
    write(
        &root.join("package.json"),
        r#"{ "name": "loja", "scripts": { "build": "tsc", "start": "ng serve" } }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "outDir": "saida" }, "include": ["fonte/**/*"] }"#,
    );

    let adapter = NpmAdapter::new(std::sync::Arc::new(NativeProcessSupervisor::default()));
    let Ok(Some(descriptor)) = pollster::block_on(adapter.detect_project(&root)) else {
        panic!("um package.json precisa ser reconhecido");
    };
    assert_eq!(descriptor.name, Some("loja".to_owned()));

    let Ok(model) = pollster::block_on(adapter.import_project(ProjectImportRequest::new(descriptor)))
    else {
        panic!("a importação precisa funcionar");
    };
    assert_eq!(model.modules.len(), 1);
    assert_eq!(model.modules[0].source_roots.main, vec![root.join("fonte")]);
    assert_eq!(model.modules[0].output_directory, root.join("saida"));

    // Os scripts viram propriedades do modelo: `ng serve` aparece porque está
    // escrito no arquivo, e não porque alguém aqui saiba o que `ng` é.
    assert_eq!(
        model.properties.get("start").map(String::as_str),
        Some("ng serve")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Um diretório sem `package.json` não é um projeto deste adapter.
#[test]
fn a_directory_without_a_manifest_is_not_a_project() {
    let root = temporary("sem-manifesto");
    let adapter = NpmAdapter::new(std::sync::Arc::new(NativeProcessSupervisor::default()));
    assert!(matches!(
        pollster::block_on(adapter.detect_project(&root)),
        Ok(None)
    ));
    assert!(scripts(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
