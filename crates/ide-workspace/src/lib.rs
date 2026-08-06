#![doc = "Documentos, árvore, busca e filesystem do workspace."]

mod document;
mod filesystem;
mod search;
mod tree;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use ide_application::{SearchScope, WorkspacePort, WorkspacePortError};
use thiserror::Error;

pub use document::{BufferError, EditorSession, OpenDocument, TextBuffer, rewrite_occurrences};
pub use filesystem::NativeWorkspaceFileSystem;
pub use search::SearchMatch;
pub use tree::FileNode;

#[derive(Clone)]
pub struct WorkspaceService {
    filesystem: Arc<dyn WorkspacePort>,
}

impl Default for WorkspaceService {
    fn default() -> Self {
        Self::native()
    }
}

/// Até onde uma cadeia de pastas únicas é seguida numa só leitura.
///
/// Uma cadeia de pacotes real tem meia dúzia de elos. O teto não existe para
/// ela: existe para o caso em que uma ligação simbólica aponta para cima e a
/// descida deixaria de ter fim.
const PROFUNDIDADE_DA_CADEIA: usize = 32;

impl WorkspaceService {
    #[must_use]
    pub fn new(filesystem: Arc<dyn WorkspacePort>) -> Self {
        Self { filesystem }
    }

    #[must_use]
    pub fn native() -> Self {
        Self::new(Arc::new(NativeWorkspaceFileSystem))
    }

    pub fn scan(&self, root: &Path) -> Result<FileNode, WorkspaceError> {
        tree::scan(self.filesystem.as_ref(), root)
    }

    /// Todos os arquivos de uma extensão sob a raiz.
    ///
    /// Lê o filesystem, e não a árvore do Explorer: desde a `19` a árvore tem só
    /// o que foi expandido, e responder por ela devolveria uma lista incompleta
    /// **sem avisar** — que é o defeito que aquela especificação combate.
    pub fn source_files(&self, root: &Path, extension: &str) -> Vec<PathBuf> {
        let mut encontrados = Vec::new();
        self.collect_sources(root, extension, &mut encontrados);
        encontrados
    }

    fn collect_sources(&self, directory: &Path, extension: &str, output: &mut Vec<PathBuf>) {
        let Ok(filhos) = tree::children_of(self.filesystem.as_ref(), directory) else {
            return;
        };
        for filho in filhos {
            if filho.is_directory {
                self.collect_sources(&filho.path, extension, output);
            } else if filho
                .path
                .extension()
                .and_then(|valor| valor.to_str())
                .is_some_and(|valor| valor.eq_ignore_ascii_case(extension))
            {
                output.push(filho.path);
            }
        }
    }

    /// Os níveis até uma pasta, da raiz para ela, cada um com os seus filhos.
    ///
    /// Uma resposta só, e **em ordem**: quem insere na árvore sempre encontra o
    /// pai já lá. Pedir pasta a pasta obrigava a árvore a existir antes do
    /// pedido, e um nível fundo se perdia por chegar cedo demais.
    pub fn scan_path(&self, root: &Path, target: &Path) -> Vec<(PathBuf, Vec<FileNode>)> {
        let Ok(relativo) = target.strip_prefix(root) else {
            return Vec::new();
        };
        let mut niveis = Vec::new();
        let mut atual = root.to_path_buf();
        while let Ok(filhos) = tree::children_of(self.filesystem.as_ref(), &atual) {
            niveis.push((atual.clone(), filhos));
            let Some(proximo) = relativo
                .strip_prefix(atual.strip_prefix(root).unwrap_or(Path::new("")))
                .ok()
                .and_then(|resto| resto.components().next())
            else {
                break;
            };
            atual = atual.join(proximo);
            if atual == target {
                if let Ok(filhos) = tree::children_of(self.filesystem.as_ref(), &atual) {
                    niveis.push((atual.clone(), filhos));
                }
                break;
            }
        }
        niveis
    }

    /// Os níveis até uma pasta, e depois **enquanto não houver o que escolher**.
    ///
    /// Uma pasta cujo único filho é outra pasta não mostra nada ao ser aberta:
    /// mostra a próxima porta. Numa árvore de pacotes isso vira `br`, depois
    /// `com`, depois `exemplo` — um clique por porta, e nenhum deles revelando
    /// coisa alguma.
    ///
    /// Quem abriu pediu para ver o que há dentro, e o que há dentro está no fim
    /// da cadeia. Esta leitura desce até lá de uma vez.
    ///
    /// A condição é **estrutural**, e não de linguagem: "um filho só, e ele é
    /// pasta". Nada aqui sabe o que é um pacote — e é por isso que vale igual
    /// para qualquer projeto.
    pub fn scan_path_until_content(
        &self,
        root: &Path,
        target: &Path,
    ) -> Vec<(PathBuf, Vec<FileNode>)> {
        let mut niveis = self.scan_path(root, target);
        // Um teto para a descida: uma cadeia de pastas únicas é curta na
        // prática, e um limite impede que um laço de ligações simbólicas
        // transforme um clique numa varredura sem fim.
        for _ in 0..PROFUNDIDADE_DA_CADEIA {
            let Some((_, filhos)) = niveis.last() else {
                break;
            };
            let [unico] = filhos.as_slice() else {
                break;
            };
            if !unico.is_directory {
                break;
            }
            let caminho = unico.path.clone();
            let Ok(netos) = tree::children_of(self.filesystem.as_ref(), &caminho) else {
                break;
            };
            niveis.push((caminho, netos));
        }
        niveis
    }

    /// Os filhos de uma pasta, para quando ela é expandida.
    pub fn scan_children(&self, directory: &Path) -> Result<Vec<FileNode>, WorkspaceError> {
        tree::children_of(self.filesystem.as_ref(), directory)
    }

    pub fn read_document(&self, path: &Path) -> Result<String, WorkspaceError> {
        self.filesystem.read_text(path).map_err(Into::into)
    }

    #[must_use]
    pub fn modified_at(&self, path: &Path) -> Option<SystemTime> {
        self.filesystem
            .metadata(path)
            .ok()
            .and_then(|entry| entry.modified)
    }

    pub fn save_document(&self, path: &Path, contents: &str) -> Result<(), WorkspaceError> {
        self.filesystem
            .write_text(path, contents)
            .map_err(Into::into)
    }

    /// Move um arquivo dentro do workspace.
    pub fn rename_path(&self, from: &Path, to: &Path) -> Result<(), WorkspaceError> {
        self.filesystem.rename_path(from, to).map_err(Into::into)
    }

    pub fn create_directory(&self, path: &Path) -> Result<(), WorkspaceError> {
        self.filesystem.create_directory(path).map_err(Into::into)
    }

    pub fn create_file(&self, path: &Path, contents: &str) -> Result<(), WorkspaceError> {
        if self.filesystem.exists(path) {
            return Err(WorkspaceError::AlreadyExists(path.to_path_buf()));
        }
        self.filesystem
            .write_text(path, contents)
            .map_err(Into::into)
    }

    #[must_use]
    /// Busca textual pelo projeto, interrompível.
    ///
    /// **Não chame isto na thread da interface.** Medido contra um projeto real
    /// de 8 958 arquivos: 1,4 s com o cache do sistema quente e **106 s frio** —
    /// que é o estado na primeira busca depois de abrir o projeto. O limite de
    /// resultados não protege: ele para quando a lista enche, e uma consulta sem
    /// ocorrência nenhuma vai até o último arquivo. Ver `tests/large_project.rs`.
    pub fn search_content(
        &self,
        root: &FileNode,
        scope: &SearchScope,
        query: &str,
        limit: usize,
        cancel: &ide_domain::CancellationToken,
    ) -> Vec<SearchMatch> {
        search::search_content(self.filesystem.as_ref(), root, scope, query, limit, cancel)
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Port(#[from] WorkspacePortError),
    #[error("{0} já existe")]
    AlreadyExists(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    fn workspace() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "er-ide-workspace-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(root.join("modulo/src/main/java/br/com")).is_ok());
        assert!(fs::create_dir_all(root.join("docs")).is_ok());
        assert!(
            fs::write(
                root.join("modulo/src/main/java/br/com/Pedido.java"),
                "class Pedido {\n    String mensagem = \"Conteudo procurado\";\n}\n"
            )
            .is_ok()
        );
        assert!(fs::write(root.join("docs/fora.txt"), "conteudo fora\n").is_ok());
        // Uma cadeia de pastas únicas, para a leitura que desce até o conteúdo.
        assert!(fs::create_dir_all(root.join("cadeia/um/dois/tres")).is_ok());
        assert!(fs::write(root.join("cadeia/um/dois/tres/Fim.java"), "class Fim {}\n").is_ok());
        assert!(
            fs::write(
                root.join("modulo/src/main/java/br/com/ignorado.txt"),
                "conteudo procurado\n"
            )
            .is_ok()
        );
        root
    }

    #[test]
    fn service_searches_only_the_explicit_scope() {
        let root = workspace();
        let service = WorkspaceService::native();
        let tree = service.scan(&root);
        assert!(tree.is_ok());
        let Ok(tree) = tree else {
            return;
        };
        let scope = SearchScope::new(
            vec![root.join("modulo/src/main/java")],
            vec!["java".to_owned()],
        );
        let cancel = ide_domain::CancellationToken::new();
        let found = service.search_content(&tree, &scope, "CONTEUDO", 20, &cancel);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
        let empty_scope = SearchScope::default();
        assert!(
            service
                .search_content(&tree, &empty_scope, "CONTEUDO", 20, &cancel)
                .is_empty()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn create_file_never_overwrites_existing_content() {
        let root = workspace();
        let service = WorkspaceService::native();
        let file = root.join("novo.txt");
        assert!(service.create_file(&file, "primeiro").is_ok());
        assert!(matches!(
            service.create_file(&file, "segundo"),
            Err(WorkspaceError::AlreadyExists(path)) if path == file
        ));
        assert_eq!(fs::read_to_string(&file).unwrap_or_default(), "primeiro");
        let _ = fs::remove_dir_all(root);
    }

    /// Abrir uma pasta desce a cadeia inteira até achar o que escolher.
    ///
    /// Sem isto, `um`, `dois` e `tres` custam um clique cada, e nenhum deles
    /// mostra coisa alguma — só a próxima porta.
    #[test]
    fn abrir_uma_pasta_desce_ate_o_conteudo() {
        let root = workspace();
        let service = WorkspaceService::native();
        let alvo = root.join("cadeia").join("um");

        let ate_o_alvo = service.scan_path(&root, &alvo);
        let ate_o_conteudo = service.scan_path_until_content(&root, &alvo);

        assert!(
            ate_o_conteudo.len() > ate_o_alvo.len(),
            "a leitura precisa passar do alvo enquanto houver um caminho só"
        );
        assert_eq!(
            ate_o_conteudo.last().map(|(caminho, _)| caminho.clone()),
            Some(root.join("cadeia").join("um").join("dois").join("tres")),
            "a descida para no primeiro nível que tem o que escolher"
        );
        assert!(
            ate_o_conteudo
                .last()
                .is_some_and(|(_, filhos)| filhos.iter().any(|no| !no.is_directory)),
            "e esse nível traz o conteúdo junto"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// A descida para onde há escolha, e não segue por uma pasta qualquer.
    #[test]
    fn a_descida_para_quando_ha_o_que_escolher() {
        let root = workspace();
        let service = WorkspaceService::native();
        // De `modulo/src` a cadeia é única até `br/com`, que tem dois filhos.
        let alvo = root.join("modulo").join("src");
        let niveis = service.scan_path_until_content(&root, &alvo);
        assert_eq!(
            niveis.last().map(|(caminho, _)| caminho.clone()),
            Some(
                root.join("modulo")
                    .join("src")
                    .join("main")
                    .join("java")
                    .join("br")
                    .join("com")
            ),
            "para no primeiro nível com mais de uma coisa dentro"
        );

        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod medicao {
    use std::{path::Path, time::Instant};

    fn contar_recursivo(dir: &Path) -> usize {
        let Ok(entradas) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut total = 0;
        for entrada in entradas.flatten() {
            let nome = entrada.file_name();
            if matches!(nome.to_str(), Some(".git" | "target")) {
                continue;
            }
            total += 1;
            if entrada.path().is_dir() {
                total += contar_recursivo(&entrada.path());
            }
        }
        total
    }

    /// Quanto custa varrer um projeto de verdade.
    ///
    /// O caminho vem de `ER_IDE_PROJETO`, e não do disco de quem escreveu o
    /// teste. **Um caminho absoluto aqui é um teste que só uma máquina roda** —
    /// e ele não falha nas outras, ele fica mudo, que é pior: parece cobertura e
    /// não é.
    ///
    /// ```text
    /// ER_IDE_PROJETO=/caminho/do/projeto cargo test -p ide-workspace -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "medição manual; exige ER_IDE_PROJETO"]
    fn scan_time_on_a_real_project() {
        let Some(root) = std::env::var_os("ER_IDE_PROJETO").map(std::path::PathBuf::from) else {
            panic!("aponte ER_IDE_PROJETO para um projeto de verdade");
        };
        let root = root.as_path();
        assert!(root.exists(), "o projeto apontado precisa existir: {root:?}");
        let service = super::WorkspaceService::native();
        let inicio = Instant::now();
        let Ok(arvore) = service.scan(root) else {
            return;
        };
        let raso = inicio.elapsed();
        // A varredura profunda, para comparar: é o que a IDE fazia na abertura.
        fn profunda(node: &super::FileNode) -> usize {
            1 + node.children.iter().map(profunda).sum::<usize>()
        }
        let _ = profunda(&arvore);
        let inicio = Instant::now();
        let total = contar_recursivo(root);
        eprintln!("profunda equivalente: {:?}, entradas={total}", inicio.elapsed());
        fn conta(node: &super::FileNode) -> usize {
            1 + node.children.iter().map(conta).sum::<usize>()
        }
        eprintln!("raso: {raso:?}, nós={}", conta(&arvore));

        let alvo = root.join("core");
        let inicio = Instant::now();
        let niveis = service.scan_path(root, &alvo);
        eprintln!("caminho até core: {:?}, níveis={}", inicio.elapsed(), niveis.len());
    }
}
