//! O `tsconfig.json` lido como ele é escrito de verdade.
//!
//! **É daqui que saem as raízes do projeto, e não de convenção.** A ADR-027
//! decide isso: o analisador de TypeScript faz descoberta própria subindo até o
//! `tsconfig.json` mais próximo, e se o nosso modelo deduzisse `src` por palpite
//! passariam a existir duas definições de qual é o projeto. Elas discordariam em
//! monorepo, em `references`, em teste excluído do build — e discordariam **em
//! silêncio**, que é a pior forma.
//!
//! A origem única é o **arquivo**, não um processo: nós lemos o mesmo
//! `tsconfig.json` que o analisador lê. Nosso leitor é aproximado e o dele é
//! exato, e errar contra a mesma fonte é defeito com forma conhecida — enquanto
//! duas definições diferentes seriam desacordo por desenho, que nenhum teste
//! apanha porque os dois lados estão certos.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// O que o arquivo declara, já com o que ele estende resolvido.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TsConfig {
    /// Diretório do arquivo que originou esta configuração.
    pub directory: PathBuf,
    pub root_dir: Option<PathBuf>,
    pub out_dir: Option<PathBuf>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub files: Vec<PathBuf>,
    /// Outros `tsconfig` que este projeto referencia.
    pub references: Vec<PathBuf>,
    /// Para que versão da linguagem este projeto compila — `"ES2022"`.
    ///
    /// É o que decide **quais** dos `lib.*.d.ts` valem, e por isso decide o que a
    /// completação pode oferecer sem sugerir código que o build recusa. Ver a
    /// fase 7 da `25`.
    pub target: Option<String>,
    /// O `lib` explícito, que manda mais do que o `target`.
    ///
    /// Quem escreve `"lib": ["ES2020", "DOM"]` está dizendo exatamente o que
    /// existe, e o `target` deixa de opinar — é a regra do compilador.
    pub lib: Vec<String>,
    /// Raiz a partir da qual um `import` sem `./` é procurado.
    pub base_url: Option<PathBuf>,
    /// Apelidos de módulo, do `paths` do compilador.
    ///
    /// É o que faz `@spartacus/core` achar `core-libs/core/public_api` sem que
    /// exista pasta nenhuma com esse nome. Num monorepo Angular são centenas de
    /// entradas — 315 no projeto de referência da `25` —, e sem elas quase todo
    /// `import` do projeto fica sem destino.
    ///
    /// O padrão pode ter um `*`, e o que ele casa entra no lugar do `*` do
    /// destino. Cada apelido tem uma **lista** de destinos, tentados em ordem.
    pub paths: Vec<(String, Vec<String>)>,
    /// Raízes que vieram dos arquivos referenciados, já resolvidas.
    ///
    /// Um `tsconfig.json` de **solução** não tem arquivos próprios: ele declara
    /// `"files": []` e aponta para os projetos de verdade em `references`. É o
    /// formato padrão do Angular — o da raiz guarda as opções compartilhadas, o
    /// `tsconfig.app.json` tem o `include` da aplicação e o `tsconfig.spec.json`
    /// o dos testes, porque os dois compilam com opções diferentes.
    ///
    /// Sem seguir as referências, a leitura cai no padrão do compilador — "sem
    /// `include`, o projeto é o diretório" — e devolve a raiz inteira, pondo
    /// `node_modules` dentro do que a IDE considera código-fonte. Num projeto
    /// Angular isso é dezenas de milhares de arquivos.
    referenced_roots: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum TsConfigError {
    Unreadable(String),
    Invalid(String),
}

impl std::fmt::Display for TsConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(detail) | Self::Invalid(detail) => formatter.write_str(detail),
        }
    }
}

/// Diretórios onde o código deste projeto vive.
///
/// A ordem de confiança é a do próprio TypeScript: `rootDir` é explícito e
/// manda; `files` e `include` dizem por onde procurar; e não havendo nada, o
/// projeto é o diretório do arquivo, que é o padrão do compilador.
impl TsConfig {
    #[must_use]
    pub fn source_roots(&self) -> Vec<PathBuf> {
        if let Some(root) = &self.root_dir {
            return vec![self.directory.join(root)];
        }
        let mut roots = BTreeSet::new();
        for pattern in &self.include {
            roots.insert(self.directory.join(literal_prefix(pattern)));
        }
        for file in &self.files {
            if let Some(parent) = self.directory.join(file).parent() {
                roots.insert(parent.to_path_buf());
            }
        }
        // O que os projetos referenciados declaram é código-fonte deste projeto
        // tanto quanto o que ele declara por si.
        roots.extend(self.referenced_roots.iter().cloned());
        if roots.is_empty() {
            // Sem `include`, `files` nem referência, o compilador toma o
            // diretório inteiro.
            roots.insert(self.directory.clone());
        }
        let mut roots: Vec<_> = roots.into_iter().collect();
        // Uma raiz contida em outra é ruído: `src` e `src/app` juntas fariam o
        // mesmo arquivo aparecer duas vezes para quem varre.
        roots.sort();
        roots.dedup();
        let mut resultado: Vec<PathBuf> = Vec::new();
        for root in roots {
            if resultado.iter().any(|anterior| root.starts_with(anterior)) {
                continue;
            }
            resultado.push(root);
        }
        resultado
    }

    /// Diretórios que a varredura não deve visitar.
    ///
    /// Ao `exclude` declarado somam-se os padrões que o próprio TypeScript
    /// assume quando ele está ausente, e a saída de compilação — que é código
    /// gerado, e indexá-lo faria cada símbolo aparecer duas vezes.
    #[must_use]
    pub fn excluded(&self) -> Vec<PathBuf> {
        let mut excluded: Vec<PathBuf> = self
            .exclude
            .iter()
            .map(|pattern| self.directory.join(literal_prefix(pattern)))
            .collect();
        if self.exclude.is_empty() {
            for padrao in DEFAULT_EXCLUDE {
                excluded.push(self.directory.join(padrao));
            }
        }
        if let Some(out) = &self.out_dir {
            excluded.push(self.directory.join(out));
        }
        excluded.sort();
        excluded.dedup();
        excluded
    }
}

const DEFAULT_EXCLUDE: &[&str] = &["node_modules", "bower_components", "jspm_packages"];

/// Lê o arquivo e tudo o que ele estende, do mais distante para o mais próximo.
pub fn load(path: &Path) -> Result<TsConfig, TsConfigError> {
    let mut visited = BTreeSet::new();
    load_with(path, &mut visited)
}

fn load_with(path: &Path, visited: &mut BTreeSet<PathBuf>) -> Result<TsConfig, TsConfigError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical) {
        // `extends` em círculo. O TypeScript recusa; nós paramos e devolvemos o
        // que já temos, porque travar a abertura do projeto seria pior.
        return Ok(TsConfig::default());
    }
    let source = std::fs::read_to_string(path)
        .map_err(|error| TsConfigError::Unreadable(error.to_string()))?;
    let value: serde_json::Value = serde_json::from_str(&strip_comments(&source))
        .map_err(|error| TsConfigError::Invalid(error.to_string()))?;
    let directory = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    // O que se estende vale como base, e o que está aqui vence por cima. Cada
    // arquivo resolve os próprios caminhos relativos ao lugar onde ele está —
    // é a regra do TypeScript, e ignorá-la quebraria justamente o caso de um
    // `tsconfig.base.json` uma pasta acima.
    let mut config = match value.get("extends").and_then(serde_json::Value::as_str) {
        Some(base) => {
            let base_path = resolve_extends(&directory, base);
            let mut herdado = load_with(&base_path, visited)?;
            herdado.directory = directory.clone();
            herdado
        }
        None => TsConfig {
            directory: directory.clone(),
            ..TsConfig::default()
        },
    };
    config.directory = directory;

    if let Some(options) = value.get("compilerOptions") {
        if let Some(root) = options.get("rootDir").and_then(serde_json::Value::as_str) {
            config.root_dir = Some(PathBuf::from(root));
        }
        if let Some(out) = options.get("outDir").and_then(serde_json::Value::as_str) {
            config.out_dir = Some(PathBuf::from(out));
        }
        // `target` e `lib` **substituem** o que veio da base, como no compilador:
        // um `tsconfig.spec.json` que declara o próprio alvo não soma ao do
        // `tsconfig.base.json`, ele o troca.
        if let Some(alvo) = options.get("target").and_then(serde_json::Value::as_str) {
            config.target = Some(alvo.to_owned());
        }
        if let Some(itens) = options.get("lib").and_then(serde_json::Value::as_array) {
            config.lib = itens
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect();
        }
        // `baseUrl` e `paths` são resolvidos **relativos ao arquivo que os
        // declara**, e não ao que o estende. É a regra do TypeScript, e ignorá-la
        // quebra o caso comum de um `tsconfig.base.json` uma pasta acima.
        if let Some(base) = options.get("baseUrl").and_then(serde_json::Value::as_str) {
            config.base_url = Some(config.directory.join(base));
        }
        if let Some(mapa) = options.get("paths").and_then(serde_json::Value::as_object) {
            config.paths = mapa
                .iter()
                .map(|(padrao, destinos)| {
                    let destinos = destinos
                        .as_array()
                        .map(|itens| {
                            itens
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    (padrao.clone(), destinos)
                })
                .collect();
        }
    }
    // `include`, `exclude` e `files` **substituem** o que veio da base, e não se
    // somam a ele. É como o TypeScript trata, e somar faria um projeto herdar
    // pastas que ele declarou não querer.
    if let Some(include) = string_list(&value, "include") {
        config.include = include;
    }
    if let Some(exclude) = string_list(&value, "exclude") {
        config.exclude = exclude;
    }
    if let Some(files) = string_list(&value, "files") {
        config.files = files.into_iter().map(PathBuf::from).collect();
    }
    config.references = value
        .get("references")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();

    // As referências são seguidas **na leitura**, e não ao perguntar pelas
    // raízes: assim `source_roots` continua sem tocar em disco, e o custo é pago
    // uma vez. O mesmo conjunto de visitados protege contra ciclo.
    let referencias = config.references.clone();
    for referencia in referencias {
        let caminho = resolve_reference(&config.directory, &referencia);
        let Ok(referenciado) = load_with(&caminho, visited) else {
            tracing::warn!(caminho = %caminho.display(), "projeto referenciado não pôde ser lido");
            continue;
        };
        config.referenced_roots.extend(referenciado.source_roots());
    }
    config.referenced_roots.sort();
    config.referenced_roots.dedup();
    Ok(config)
}

/// Onde mora o arquivo estendido.
///
/// Caminho relativo é relativo a quem estende. Sem `./` na frente, o TypeScript
/// procura em `node_modules` — é assim que `@tsconfig/node20/tsconfig.json`
/// funciona.
fn resolve_extends(directory: &Path, base: &str) -> PathBuf {
    let candidate = if base.starts_with('.') {
        directory.join(base)
    } else {
        directory.join("node_modules").join(base)
    };
    if candidate.extension().is_some() {
        candidate
    } else {
        candidate.join("tsconfig.json")
    }
}

/// Onde mora um projeto referenciado.
///
/// O caminho pode apontar para o arquivo ou para a pasta que o contém — as duas
/// formas são válidas, e o Angular usa a primeira.
fn resolve_reference(directory: &Path, referencia: &Path) -> PathBuf {
    let candidato = directory.join(referencia);
    if candidato.extension().is_some() {
        candidato
    } else {
        candidato.join("tsconfig.json")
    }
}

fn string_list(value: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    Some(
        value
            .get(key)?
            .as_array()?
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
    )
}

/// A parte do padrão que é caminho de verdade, antes do primeiro curinga.
///
/// `src/**/*.ts` vira `src`; `**/*` vira o próprio diretório. Não é expansão de
/// glob: é o prefixo literal, que é o que uma raiz precisa ser.
fn literal_prefix(pattern: &str) -> PathBuf {
    let mut prefix = PathBuf::new();
    for parte in pattern.split(['/', '\\']) {
        if parte.contains('*') || parte.contains('?') || parte.is_empty() {
            break;
        }
        prefix.push(parte);
    }
    prefix
}

/// Remove comentários e vírgula sobrando, respeitando o que está entre aspas.
///
/// O `tsconfig.json` não é JSON: aceita `//`, `/* */` e vírgula final, e todo
/// projeto gerado pela CLI do TypeScript vem cheio de comentários explicando as
/// opções. Um leitor de JSON estrito recusaria o arquivo padrão da ferramenta.
fn strip_comments(source: &str) -> String {
    let mut saida = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut em_texto = false;
    let mut escapado = false;
    while let Some(atual) = chars.next() {
        if em_texto {
            saida.push(atual);
            if escapado {
                escapado = false;
            } else if atual == '\\' {
                escapado = true;
            } else if atual == '"' {
                em_texto = false;
            }
            continue;
        }
        match atual {
            '"' => {
                em_texto = true;
                saida.push(atual);
            }
            '/' if chars.peek() == Some(&'/') => {
                for seguinte in chars.by_ref() {
                    if seguinte == '\n' {
                        saida.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut anterior = '\0';
                for seguinte in chars.by_ref() {
                    if anterior == '*' && seguinte == '/' {
                        break;
                    }
                    anterior = seguinte;
                }
            }
            _ => saida.push(atual),
        }
    }
    remove_trailing_commas(&saida)
}

fn remove_trailing_commas(source: &str) -> String {
    let mut saida = String::with_capacity(source.len());
    let mut em_texto = false;
    let mut escapado = false;
    let bytes: Vec<char> = source.chars().collect();
    for (indice, atual) in bytes.iter().enumerate() {
        if em_texto {
            saida.push(*atual);
            if escapado {
                escapado = false;
            } else if *atual == '\\' {
                escapado = true;
            } else if *atual == '"' {
                em_texto = false;
            }
            continue;
        }
        if *atual == '"' {
            em_texto = true;
            saida.push(*atual);
            continue;
        }
        if *atual == ',' {
            let proximo = bytes[indice + 1..]
                .iter()
                .find(|candidato| !candidato.is_whitespace());
            if matches!(proximo, Some(']' | '}')) {
                continue;
            }
        }
        saida.push(*atual);
    }
    saida
}
