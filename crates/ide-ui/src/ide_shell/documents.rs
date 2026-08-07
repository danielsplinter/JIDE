//! A fachada de documentos: abrir, salvar, e o que está no editor agora.
//!
//! É por aqui que a aplicação fala com o shell sobre arquivos. Nada aqui
//! desenha nem responde a gesto.

use super::*;

impl IdeShell {
    /// Raiz atualmente carregada no Explorer.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        self.explorer.workspace_root()
    }

    /// Árvore já varrida que serviços de workspace podem consultar sem refazer
    /// I/O a cada tecla.
    #[must_use]
    pub fn workspace_tree(&self) -> &FileNode {
        self.explorer.workspace_tree()
    }

    /// Substitui a árvore já carregada pela camada de workspace.
    ///
    /// A varredura é rasa: a raiz vem com um nível só. O que estava expandido
    /// precisa ser relido, ou recarregar o workspace faria sumir tudo o que o
    /// usuário tinha aberto — inclusive um arquivo recém-criado lá dentro.
    pub fn replace_workspace_tree(&mut self, workspace: FileNode) {
        self.explorer
            .replace_workspace(
                workspace,
                &self.catalog.source_root_names,
                &self.declaration_kinds,
            );
        self.request_expanded_directories();
        // A `TreeView` guarda os itens dela: reler o disco sem repô-los deixava a
        // árvore desenhando a varredura anterior. `set_roots` preserva expansão e
        // seleção por identidade, então a posição do usuário não se perde.
        self.sync_explorer_tree();
    }

    /// Revisão do documento ativo, para quem precisa notar que o texto mudou.
    ///
    /// Um clique pode alterar o texto — gerar acessores, por exemplo —, e quem
    /// mantém o realce precisa perceber isso sem depender de uma tecla.
    #[must_use]
    pub fn active_revision(&self) -> u64 {
        self.editor_area
            .session
            .active()
            .map_or(0, |document| document.buffer.revision())
    }

    /// A aba ativa tem alteração ainda não gravada.
    ///
    /// É o que a marca na aba anuncia, e o que decide se fechar sem salvar
    /// perderia trabalho.
    #[must_use]
    pub fn active_document_modified(&self) -> bool {
        self.editor_area
            .session
            .active()
            .is_some_and(|document| document.buffer.is_dirty())
    }

    /// Solicita a gravação da aba ativa à camada de aplicação.
    pub fn request_save_active_document(&mut self) {
        let Some(document) = self.editor_area.session.active() else {
            self.context.status_message = "Nenhum documento aberto".to_owned();
            return;
        };
        if !document.is_persistent() {
            self.context.status_message =
                "Documento em memória não possui caminho para salvar".to_owned();
            return;
        }
        self.commands
            .push(ApplicationCommand::SaveDocument(SaveDocumentRequest {
                document_id: document.id,
                path: document.path.clone(),
                text: document.buffer.text().to_owned(),
                revision: document.buffer.revision(),
            }));
    }

    pub fn document_saved(&mut self, document_id: DocumentId, revision: u64, path: &Path) {
        if self
            .editor_area
            .session
            .mark_saved(document_id, revision)
            .is_ok()
        {
            self.context.status_message = format!("Salvo {}", path.display());
        }
    }

    /// Apresenta um documento cujo conteúdo já foi carregado pelo workspace.
    pub fn show_document(&mut self, path: &Path, text: impl Into<String>) -> DocumentId {
        let id = self.editor_area.session.open(path, text);
        // Com a área dividida, o arquivo abre no painel em que se está olhando.
        // Abrir sempre à esquerda faria a divisão servir para ver dois arquivos
        // só até alguém clicar no terceiro.
        if !self.abrir_no_split(id) {
            self.editor_area.pane.set_cursor(0);
        }
        self.context.focus = ShellFocus::Editor;
        self.context.status_message = format!("Opened {}", path.display());
        self.sync_explorer_to_active();
        id
    }

    pub const fn active_document(&self) -> Option<DocumentId> {
        self.editor_area.active_document()
    }

    pub fn active_text(&self) -> Option<&str> {
        self.editor_area.active_text()
    }

    /// Os documentos que a camada de linguagem precisa conhecer.
    ///
    /// As abas abertas, **e os dois lados da comparação do Git quando há uma na
    /// tela**. Os dois lados não são abas — o arquivo de então nem existe no
    /// disco —, mas precisam de realce pelo mesmo motivo que o editor: um
    /// `diff` sem cor obriga a ler com mais atenção justamente onde a atenção já
    /// está ocupada com o que mudou.
    ///
    /// Entrar por aqui é o que lhes dá **o ciclo de vida inteiro de graça**:
    /// abrem quando a comparação abre, mudam quando ela troca de arquivo e são
    /// fechados quando ela sai da tela, porque quem some desta lista é fechado.
    pub fn document_snapshots(&self) -> Vec<DocumentSnapshot> {
        let mut snapshots: Vec<DocumentSnapshot> = self
            .editor_area
            .session
            .tabs()
            .map(|document| DocumentSnapshot {
                id: document.id,
                path: document.path.clone(),
                version: document.buffer.revision(),
                text: document.buffer.text().to_owned(),
            })
            .collect();
        snapshots.extend(self.git.textos_do_diff().into_iter().map(
            |(id, path, version, text)| DocumentSnapshot {
                id,
                path,
                version,
                text,
            },
        ));
        snapshots
    }

    pub fn set_syntax_snapshot(&mut self, snapshot: SyntaxSnapshot) {
        // Os dois lados da comparação não são documentos abertos, e o realce
        // deles não é do editor: vai para a janela que os desenha.
        if self.git.aceita_realce(snapshot.document_id) {
            self.instalar_realce_do_diff(&snapshot);
            return;
        }
        let Some(document) = self.editor_area.session.document(snapshot.document_id) else {
            return;
        };
        let spans = converted_syntax(document.buffer.text(), &snapshot);
        let error_count = snapshot.diagnostics.len();
        let symbol_count = count_outline(&snapshot.outline);
        let import_count = snapshot.imports.len();
        let language = self
            .catalog
            .language_names
            .first()
            .map_or("Análise", String::as_str);
        self.context.status_message = format!(
            "{language}: {error_count} error(s), {symbol_count} symbol(s), {import_count} import(s)"
        );
        self.editor_area.syntax_spans.insert(
            snapshot.document_id,
            CachedSyntax {
                version: snapshot.version,
                spans,
            },
        );
        self.editor_area
            .syntax_snapshots
            .insert(snapshot.document_id, snapshot);
    }

    /// Reparte o realce de um lado da comparação e o entrega à janela.
    ///
    /// A conversão é a mesma do editor — posições em linha e coluna viram
    /// deslocamentos no texto —, e por isso mora no mesmo lugar: duas contas
    /// iguais escritas em dois lugares divergem na primeira correção.
    fn instalar_realce_do_diff(&mut self, snapshot: &SyntaxSnapshot) {
        let de_agora = self.git.realce_e_do_lado_de_agora(snapshot.document_id);
        let Some(texto) = self.git.texto_do_lado(de_agora) else {
            return;
        };
        let spans = converted_syntax(&texto, snapshot)
            .into_iter()
            .map(|(start, end, kind)| git::GitToken { start, end, kind })
            .collect::<Vec<_>>();
        self.git.set_realce_do_diff(de_agora, &texto, &spans);
    }

    pub fn syntax_snapshot(&self, document_id: DocumentId) -> Option<&SyntaxSnapshot> {
        self.editor_area.syntax_snapshots.get(&document_id)
    }

    pub fn active_outline(&self) -> &[OutlineItem] {
        self.active_document()
            .and_then(|id| self.editor_area.syntax_snapshots.get(&id))
            .map_or(&[], |snapshot| snapshot.outline.as_slice())
    }

    pub fn source_files(&self, expected_extension: &str) -> Vec<PathBuf> {
        fn collect(node: &FileNode, expected_extension: &str, output: &mut Vec<PathBuf>) {
            if node.is_directory {
                for child in &node.children {
                    collect(child, expected_extension, output);
                }
            } else if node
                .path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case(expected_extension))
            {
                output.push(node.path.clone());
            }
        }
        let mut files = Vec::new();
        collect(&self.explorer.workspace, expected_extension, &mut files);
        files
    }

    /// Caminhos das abas abertas, na ordem em que aparecem.
    ///
    /// Documentos criados em memória não têm arquivo por trás e ficam de fora:
    /// registrar um caminho que não existe só produziria uma aba impossível de
    /// reabrir.
    #[must_use]
    pub fn open_document_paths(&self) -> Vec<PathBuf> {
        self.editor_area
            .session
            .tabs()
            .filter(|document| document.is_persistent())
            .map(|document| document.path.clone())
            .collect()
    }

    /// Faz a aba seguir o arquivo que mudou de nome.
    ///
    /// Sem isso a aba continuaria apontando para um caminho que não existe
    /// mais, e a próxima gravação recriaria o arquivo antigo.
    pub fn follow_renamed_path(&mut self, from: &Path, to: &Path) {
        let aberto = self
            .editor_area
            .session
            .tabs()
            .find(|documento| documento.path == from)
            .map(|documento| documento.id);
        if let Some(id) = aberto {
            self.editor_area.session.set_path(id, to.to_path_buf());
        }
    }

    /// Caminho do documento em foco.
    #[must_use]
    pub fn active_document_path(&self) -> Option<PathBuf> {
        self.editor_area
            .session
            .active()
            .filter(|document| document.is_persistent())
            .map(|document| document.path.clone())
    }
}
