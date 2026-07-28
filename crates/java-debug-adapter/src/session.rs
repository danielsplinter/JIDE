//! Sessão de depuração conectada a um processo Java em execução.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use ide_debug_api::{
    BreakpointId, DebugError, DebugEvent, DebugEventSink, DebugSession, FrameId,
    ResolvedBreakpoint, SourceBreakpoint, StackFrame, StepKind, StopReason, ThreadDescriptor,
    ThreadId, Variable,
};
use ide_domain::{Location, TextPosition, TextRange};
use tokio::sync::Mutex;

use crate::{
    connection::Connection,
    resolve::{self, LineEntry},
    values::{self, type_name},
    wire::{
        Encoder, JdwpLocation, Value, command, command_set, event_kind, invoke, modifier, step,
        suspend_policy,
    },
};

#[derive(Clone, Debug)]
struct MethodInfo {
    id: u64,
    name: String,
    /// Assinatura JNI, que diz quantos argumentos o método recebe.
    ///
    /// Sobrecarga é a regra em Java, e o nome sozinho não escolhe qual método
    /// chamar: a quantidade de argumentos separa a maioria dos casos.
    signature: String,
}

#[derive(Clone, Debug)]
struct InstalledBreakpoint {
    id: BreakpointId,
    requested: SourceBreakpoint,
    verified_line: Option<u32>,
    message: Option<String>,
    requests: Vec<i32>,
}

impl InstalledBreakpoint {
    fn resolved(&self) -> ResolvedBreakpoint {
        ResolvedBreakpoint {
            id: self.id,
            requested: self.requested.clone(),
            verified_line: self.verified_line,
            message: self.message.clone(),
        }
    }
}

#[derive(Default)]
struct State {
    files: HashMap<PathBuf, Vec<InstalledBreakpoint>>,
    /// Requisição JDWP de breakpoint para o breakpoint da IDE que a originou.
    breakpoint_requests: HashMap<i32, BreakpointId>,
    /// Um pedido de `ClassPrepare` por classe observada.
    class_prepare: HashMap<String, i32>,
    step_requests: HashSet<i32>,
    suspended: HashSet<u64>,
    next_breakpoint: u64,
    detached: bool,
}

#[derive(Default)]
struct Cache {
    methods: HashMap<u64, Arc<Vec<MethodInfo>>>,
    lines: HashMap<(u64, u64), Arc<Vec<LineEntry>>>,
    signatures: HashMap<u64, String>,
    /// Localização de cada quadro visto na última pilha lida.
    frames: HashMap<u64, JdwpLocation>,
}

pub(crate) struct Session {
    connection: Arc<Connection>,
    source_roots: Vec<PathBuf>,
    events: Arc<dyn DebugEventSink>,
    state: Mutex<State>,
    cache: Mutex<Cache>,
}

impl Session {
    pub(crate) fn new(
        connection: Arc<Connection>,
        source_roots: Vec<PathBuf>,
        events: Arc<dyn DebugEventSink>,
    ) -> Self {
        Self {
            connection,
            source_roots,
            events,
            state: Mutex::new(State::default()),
            cache: Mutex::new(Cache::default()),
        }
    }

    pub(crate) async fn version(&self) -> Result<String, DebugError> {
        let payload = self
            .connection
            .request(
                command_set::VIRTUAL_MACHINE,
                command::VM_VERSION,
                Vec::new(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let description = decoder.string()?;
        Ok(description
            .lines()
            .next()
            .unwrap_or("JVM")
            .trim()
            .to_owned())
    }

    // ---------------------------------------------------------------- classes

    async fn classes_matching(&self, prefix: &str) -> Result<Vec<(u8, u64, String)>, DebugError> {
        let payload = self
            .connection
            .request(
                command_set::VIRTUAL_MACHINE,
                command::VM_ALL_CLASSES,
                Vec::new(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let count = decoder.i32()?.max(0);
        let mut classes = Vec::new();
        for _ in 0..count {
            let tag = decoder.u8()?;
            let id = decoder.reference_type_id()?;
            let signature = decoder.string()?;
            let _status = decoder.i32()?;
            if resolve::signature_matches(&signature, prefix) {
                classes.push((tag, id, signature));
            }
        }
        Ok(classes)
    }

    async fn methods(&self, class_id: u64) -> Result<Arc<Vec<MethodInfo>>, DebugError> {
        if let Some(cached) = self.cache.lock().await.methods.get(&class_id) {
            return Ok(Arc::clone(cached));
        }
        let mut encoder = self.connection.encoder();
        encoder.reference_type_id(class_id);
        let payload = self
            .connection
            .request(
                command_set::REFERENCE_TYPE,
                command::REFERENCE_TYPE_METHODS,
                encoder.finish(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let count = decoder.i32()?.max(0);
        let mut methods = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let id = decoder.method_id()?;
            let name = decoder.string()?;
            let signature = decoder.string()?;
            let _modifiers = decoder.i32()?;
            methods.push(MethodInfo {
                id,
                name,
                signature,
            });
        }
        let methods = Arc::new(methods);
        self.cache
            .lock()
            .await
            .methods
            .insert(class_id, Arc::clone(&methods));
        Ok(methods)
    }

    async fn line_table(
        &self,
        class_id: u64,
        method_id: u64,
    ) -> Result<Arc<Vec<LineEntry>>, DebugError> {
        if let Some(cached) = self.cache.lock().await.lines.get(&(class_id, method_id)) {
            return Ok(Arc::clone(cached));
        }
        let mut encoder = self.connection.encoder();
        encoder.reference_type_id(class_id).method_id(method_id);
        let entries = match self
            .connection
            .request(
                command_set::METHOD,
                command::METHOD_LINE_TABLE,
                encoder.finish(),
            )
            .await
        {
            Ok(payload) => {
                let mut decoder = self.connection.decoder(&payload);
                let _start = decoder.i64()?;
                let _end = decoder.i64()?;
                let count = decoder.i32()?.max(0);
                let mut entries = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let index = decoder.i64()? as u64;
                    let line = decoder.i32()?;
                    entries.push(LineEntry { index, line });
                }
                entries
            }
            // Métodos nativos e abstratos não têm tabela de linhas.
            Err(DebugError::Target(_)) => Vec::new(),
            Err(error) => return Err(error),
        };
        let entries = Arc::new(entries);
        self.cache
            .lock()
            .await
            .lines
            .insert((class_id, method_id), Arc::clone(&entries));
        Ok(entries)
    }

    async fn signature(&self, class_id: u64) -> Result<String, DebugError> {
        if let Some(cached) = self.cache.lock().await.signatures.get(&class_id) {
            return Ok(cached.clone());
        }
        let mut encoder = self.connection.encoder();
        encoder.reference_type_id(class_id);
        let payload = self
            .connection
            .request(
                command_set::REFERENCE_TYPE,
                command::REFERENCE_TYPE_SIGNATURE,
                encoder.finish(),
            )
            .await?;
        let signature = self.connection.decoder(&payload).string()?;
        self.cache
            .lock()
            .await
            .signatures
            .insert(class_id, signature.clone());
        Ok(signature)
    }

    // ------------------------------------------------------------ breakpoints

    async fn clear_request(&self, kind: u8, request_id: i32) {
        let mut encoder = self.connection.encoder();
        encoder.u8(kind).i32(request_id);
        let _ = self
            .connection
            .request(
                command_set::EVENT_REQUEST,
                command::EVENT_REQUEST_CLEAR,
                encoder.finish(),
            )
            .await;
    }

    async fn watch_class(&self, pattern: &str) -> Result<(), DebugError> {
        if self.state.lock().await.class_prepare.contains_key(pattern) {
            return Ok(());
        }
        let mut encoder = self.connection.encoder();
        encoder
            .u8(event_kind::CLASS_PREPARE)
            .u8(suspend_policy::EVENT_THREAD)
            .i32(1)
            .u8(modifier::CLASS_MATCH)
            .string(pattern);
        let payload = self
            .connection
            .request(
                command_set::EVENT_REQUEST,
                command::EVENT_REQUEST_SET,
                encoder.finish(),
            )
            .await?;
        let request_id = self.connection.decoder(&payload).i32()?;
        self.state
            .lock()
            .await
            .class_prepare
            .insert(pattern.to_owned(), request_id);
        Ok(())
    }

    async fn install(&self, location: JdwpLocation) -> Result<i32, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder
            .u8(event_kind::BREAKPOINT)
            .u8(suspend_policy::EVENT_THREAD)
            .i32(1)
            .u8(modifier::LOCATION_ONLY)
            .location(location);
        let payload = self
            .connection
            .request(
                command_set::EVENT_REQUEST,
                command::EVENT_REQUEST_SET,
                encoder.finish(),
            )
            .await?;
        self.connection.decoder(&payload).i32()
    }

    /// Instala um breakpoint em todas as classes já carregadas do arquivo.
    async fn install_in_classes(
        &self,
        classes: &[(u8, u64, String)],
        line: u32,
    ) -> Result<(Vec<i32>, Option<u32>, Option<String>), DebugError> {
        let target_line = line as i32 + 1;
        let mut requests = Vec::new();
        let mut verified: Option<i32> = None;
        for (tag, class_id, _) in classes {
            for method in self.methods(*class_id).await?.iter() {
                let entries = self.line_table(*class_id, method.id).await?;
                let Some(entry) = resolve::best_entry(&entries, target_line) else {
                    continue;
                };
                // Uma linha só pertence a um método; a primeira correspondência
                // exata encerra a busca nesta classe.
                let request = self
                    .install(JdwpLocation {
                        tag: *tag,
                        class_id: *class_id,
                        method_id: method.id,
                        index: entry.index,
                    })
                    .await?;
                requests.push(request);
                verified = Some(verified.map_or(entry.line, |current| current.min(entry.line)));
                if entry.line == target_line {
                    break;
                }
            }
        }
        let message = match verified {
            None => Some("classe ainda não carregada pelo alvo".to_owned()),
            Some(line) if line != target_line => {
                Some(format!("movido para a linha executável {line}"))
            }
            Some(_) => None,
        };
        Ok((
            requests,
            verified.map(|line| (line - 1).max(0) as u32),
            message,
        ))
    }

    /// Reavalia os breakpoints de um arquivo quando sua classe é carregada.
    async fn install_pending(&self, signature: &str) -> Result<(), DebugError> {
        let Some(path) = resolve::source_path(signature, &self.source_roots) else {
            return Ok(());
        };
        let pending: Vec<InstalledBreakpoint> = {
            let state = self.state.lock().await;
            state
                .files
                .get(&path)
                .map(|breakpoints| {
                    breakpoints
                        .iter()
                        .filter(|breakpoint| breakpoint.verified_line.is_none())
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        };
        if pending.is_empty() {
            return Ok(());
        }
        let Some(relative) = resolve::relative_source(&path, &self.source_roots) else {
            return Ok(());
        };
        let Some(fully_qualified) = resolve::fully_qualified_name(&relative) else {
            return Ok(());
        };
        let classes = self
            .classes_matching(&resolve::signature_prefix(&fully_qualified))
            .await?;
        for breakpoint in pending {
            let (requests, verified_line, message) = self
                .install_in_classes(&classes, breakpoint.requested.line)
                .await?;
            if requests.is_empty() {
                continue;
            }
            let mut state = self.state.lock().await;
            for request in &requests {
                state.breakpoint_requests.insert(*request, breakpoint.id);
            }
            if let Some(stored) = state
                .files
                .get_mut(&path)
                .and_then(|file| file.iter_mut().find(|stored| stored.id == breakpoint.id))
            {
                stored.requests.extend(requests);
                stored.verified_line = verified_line;
                stored.message = message;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ pilha

    async fn frames(&self, thread: u64) -> Result<Vec<(u64, JdwpLocation)>, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder.object_id(thread).i32(0).i32(-1);
        let payload = self
            .connection
            .request(
                command_set::THREAD_REFERENCE,
                command::THREAD_FRAMES,
                encoder.finish(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let count = decoder.i32()?.max(0);
        let mut frames = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let id = decoder.frame_id()?;
            let location = decoder.location()?;
            frames.push((id, location));
        }
        let mut cache = self.cache.lock().await;
        for (id, location) in &frames {
            cache.frames.insert(*id, *location);
        }
        Ok(frames)
    }

    async fn location_of(&self, location: JdwpLocation) -> Option<Location> {
        let signature = self.signature(location.class_id).await.ok()?;
        let path = resolve::source_path(&signature, &self.source_roots)?;
        let entries = self
            .line_table(location.class_id, location.method_id)
            .await
            .ok()?;
        let line = entries
            .iter()
            .filter(|entry| entry.index <= location.index)
            .max_by_key(|entry| entry.index)
            .map(|entry| entry.line)
            .unwrap_or(1);
        let line = (line - 1).max(0) as u32;
        Some(Location {
            path,
            range: TextRange {
                start: TextPosition { line, column: 0 },
                end: TextPosition { line, column: 0 },
            },
        })
    }

    async fn frame_location(&self, thread: u64, frame: u64) -> Result<JdwpLocation, DebugError> {
        if let Some(location) = self.cache.lock().await.frames.get(&frame) {
            return Ok(*location);
        }
        self.frames(thread).await?;
        self.cache
            .lock()
            .await
            .frames
            .get(&frame)
            .copied()
            .ok_or_else(|| DebugError::Protocol(format!("frame {frame} is unknown")))
    }

    // -------------------------------------------------------------- variáveis

    async fn variable_slots(
        &self,
        location: JdwpLocation,
    ) -> Result<Vec<(String, String, i32)>, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder
            .reference_type_id(location.class_id)
            .method_id(location.method_id);
        let payload = match self
            .connection
            .request(
                command_set::METHOD,
                command::METHOD_VARIABLE_TABLE,
                encoder.finish(),
            )
            .await
        {
            Ok(payload) => payload,
            // Sem `-g` o alvo não guarda nomes de variáveis locais.
            Err(DebugError::Target(_)) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut decoder = self.connection.decoder(&payload);
        let _argument_count = decoder.i32()?;
        let count = decoder.i32()?.max(0);
        let mut slots = Vec::new();
        for _ in 0..count {
            let code_index = decoder.i64()? as u64;
            let name = decoder.string()?;
            let signature = decoder.string()?;
            let length = decoder.i32()?.max(0) as u64;
            let slot = decoder.i32()?;
            if location.index >= code_index && location.index < code_index + length {
                slots.push((name, signature, slot));
            }
        }
        Ok(slots)
    }

    async fn slot_values(
        &self,
        thread: u64,
        frame: u64,
        slots: &[(String, String, i32)],
    ) -> Result<Vec<Value>, DebugError> {
        if slots.is_empty() {
            return Ok(Vec::new());
        }
        let mut encoder = self.connection.encoder();
        encoder
            .object_id(thread)
            .frame_id(frame)
            .i32(slots.len() as i32);
        for (_, signature, slot) in slots {
            let tag = signature.as_bytes().first().copied().unwrap_or(b'L');
            encoder.i32(*slot).u8(if tag == b'[' { b'[' } else { tag });
        }
        let payload = self
            .connection
            .request(
                command_set::STACK_FRAME,
                command::STACK_FRAME_GET_VALUES,
                encoder.finish(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let count = decoder.i32()?.max(0);
        let mut values = Vec::with_capacity(count as usize);
        for _ in 0..count {
            values.push(decoder.tagged_value()?);
        }
        Ok(values)
    }

    async fn this_object(&self, thread: u64, frame: u64) -> Result<Option<u64>, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder.object_id(thread).frame_id(frame);
        let payload = self
            .connection
            .request(
                command_set::STACK_FRAME,
                command::STACK_FRAME_THIS_OBJECT,
                encoder.finish(),
            )
            .await?;
        match self.connection.decoder(&payload).tagged_value()? {
            Value::Object { id, .. } if id != 0 => Ok(Some(id)),
            _ => Ok(None),
        }
    }

    async fn string_value(&self, id: u64) -> Result<String, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder.object_id(id);
        let payload = self
            .connection
            .request(
                command_set::STRING_REFERENCE,
                command::STRING_VALUE,
                encoder.finish(),
            )
            .await?;
        self.connection.decoder(&payload).string()
    }

    async fn array_length(&self, id: u64) -> Result<i32, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder.object_id(id);
        let payload = self
            .connection
            .request(
                command_set::ARRAY_REFERENCE,
                command::ARRAY_LENGTH,
                encoder.finish(),
            )
            .await?;
        self.connection.decoder(&payload).i32()
    }

    async fn object_class(&self, id: u64) -> Result<u64, DebugError> {
        let mut encoder = self.connection.encoder();
        encoder.object_id(id);
        let payload = self
            .connection
            .request(
                command_set::OBJECT_REFERENCE,
                command::OBJECT_REFERENCE_TYPE,
                encoder.finish(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let _tag = decoder.u8()?;
        decoder.reference_type_id()
    }

    async fn present(&self, name: &str, value: &Value) -> Variable {
        let (text, type_name, expandable) = self.describe(value).await;
        Variable {
            name: name.to_owned(),
            value: text,
            type_name,
            expandable,
        }
    }

    async fn describe(&self, value: &Value) -> (String, Option<String>, bool) {
        if let Some(text) = values::format_primitive(value) {
            return (text, None, false);
        }
        if values::is_null(value) {
            return ("null".to_owned(), None, false);
        }
        let Value::Object { id, .. } = value else {
            return ("?".to_owned(), None, false);
        };
        if values::is_string(value) {
            let text = self
                .string_value(*id)
                .await
                .unwrap_or_else(|_| "<string>".to_owned());
            return (format!("\"{text}\""), Some("String".to_owned()), false);
        }
        let signature = match self.object_class(*id).await {
            Ok(class_id) => self.signature(class_id).await.unwrap_or_default(),
            Err(_) => String::new(),
        };
        let name = type_name(&signature);
        if values::is_array(value) {
            let length = self.array_length(*id).await.unwrap_or(0);
            return (format!("{name}[{length}]"), Some(name), false);
        }
        (format!("{name}@{id}"), Some(name), true)
    }

    /// Executa um método do objeto endereçado pelo caminho.
    ///
    /// A chamada roda **dentro do processo depurado**, com a mesma thread que
    /// está parada — e é essa a diferença entre inspecionar e executar. A thread
    /// escolhida é a única a rodar durante a chamada: retomar a VM inteira faria
    /// o resto do programa avançar enquanto o usuário olha o estado parado.
    async fn invoke(
        &self,
        thread: u64,
        frame: u64,
        call: &values::MethodCall,
    ) -> Result<Value, DebugError> {
        let receiver = if call.receiver.is_empty() {
            self.this_object(thread, frame).await?.ok_or_else(|| {
                DebugError::Unsupported("quadro estático não tem `this`".to_owned())
            })?
        } else {
            match self.resolve_path(thread, frame, &call.receiver).await? {
                Value::Object { id, .. } if id != 0 => id,
                Value::Object { .. } => {
                    return Err(DebugError::Unsupported(format!(
                        "{} é null",
                        call.receiver.join(".")
                    )));
                }
                _ => {
                    return Err(DebugError::Unsupported(
                        "só objetos recebem chamada de método".to_owned(),
                    ));
                }
            }
        };
        let class_id = self.object_class(receiver).await?;
        let (declaring, method) = self
            .find_method(class_id, &call.method, call.arguments.len())
            .await?;

        let mut encoder = self.connection.encoder();
        encoder.object_id(receiver);
        encoder.object_id(thread);
        encoder.reference_type_id(declaring);
        encoder.method_id(method);
        encoder.i32(call.arguments.len() as i32);
        for argument in &call.arguments {
            encode_literal(&mut encoder, argument);
        }
        encoder.i32(invoke::SINGLE_THREADED);
        let payload = self
            .connection
            .request(
                command_set::OBJECT_REFERENCE,
                command::OBJECT_INVOKE_METHOD,
                encoder.finish(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let returned = decoder.tagged_value()?;
        // A exceção vem junto do retorno, e não como erro do protocolo: sem
        // olhá-la, um método que falhou pareceria ter devolvido `null`.
        if let Ok(Value::Object { id, .. }) = decoder.tagged_value()
            && id != 0
        {
            let (text, type_name, _) = self.describe(&Value::Object { tag: b'L', id }).await;
            let nome = type_name.unwrap_or_else(|| "exceção".to_owned());
            return Err(DebugError::Protocol(format!("{nome} lançada: {text}")));
        }
        Ok(returned)
    }

    /// Procura o método pelo nome e pela quantidade de argumentos, subindo a
    /// hierarquia.
    ///
    /// Herdar é a regra: `toString` costuma estar em `Object`, não na classe do
    /// objeto. Sobrecarga com a mesma aridade não é resolvida — nesse caso a
    /// primeira encontrada vence, e o alvo recusa se os tipos não baterem.
    async fn find_method(
        &self,
        class_id: u64,
        name: &str,
        arity: usize,
    ) -> Result<(u64, u64), DebugError> {
        let mut current = class_id;
        for _ in 0..8 {
            let methods = self.methods(current).await?;
            if let Some(found) = methods
                .iter()
                .find(|method| method.name == name && signature_arity(&method.signature) == arity)
            {
                return Ok((current, found.id));
            }
            let mut encoder = self.connection.encoder();
            encoder.reference_type_id(current);
            let Ok(payload) = self
                .connection
                .request(
                    command_set::CLASS_TYPE,
                    command::CLASS_TYPE_SUPERCLASS,
                    encoder.finish(),
                )
                .await
            else {
                break;
            };
            match self.connection.decoder(&payload).reference_type_id() {
                Ok(0) | Err(_) => break,
                Ok(super_class) => current = super_class,
            }
        }
        Err(DebugError::Unsupported(format!(
            "método {name} com {arity} argumento(s) não encontrado"
        )))
    }

    /// Campos de instância da classe e de suas superclasses.
    async fn fields_of(&self, object_id: u64) -> Result<Vec<(String, String, u64)>, DebugError> {
        let mut class_id = self.object_class(object_id).await?;
        let mut fields = Vec::new();
        for _ in 0..8 {
            let mut encoder = self.connection.encoder();
            encoder.reference_type_id(class_id);
            let payload = self
                .connection
                .request(
                    command_set::REFERENCE_TYPE,
                    command::REFERENCE_TYPE_FIELDS,
                    encoder.finish(),
                )
                .await?;
            let mut decoder = self.connection.decoder(&payload);
            let count = decoder.i32()?.max(0);
            for _ in 0..count {
                let id = decoder.field_id()?;
                let name = decoder.string()?;
                let signature = decoder.string()?;
                let modifiers = decoder.i32()?;
                // Campos estáticos não pertencem à instância inspecionada.
                if modifiers & 0x0008 == 0 {
                    fields.push((name, signature, id));
                }
            }
            let mut encoder = self.connection.encoder();
            encoder.reference_type_id(class_id);
            let Ok(payload) = self
                .connection
                .request(
                    command_set::CLASS_TYPE,
                    command::CLASS_TYPE_SUPERCLASS,
                    encoder.finish(),
                )
                .await
            else {
                break;
            };
            match self.connection.decoder(&payload).reference_type_id() {
                Ok(0) | Err(_) => break,
                Ok(super_class) => class_id = super_class,
            }
        }
        Ok(fields)
    }

    async fn field_values(
        &self,
        object_id: u64,
        fields: &[(String, String, u64)],
    ) -> Result<Vec<Value>, DebugError> {
        if fields.is_empty() {
            return Ok(Vec::new());
        }
        let mut encoder = self.connection.encoder();
        encoder.object_id(object_id).i32(fields.len() as i32);
        for (_, _, id) in fields {
            encoder.field_id(*id);
        }
        let payload = self
            .connection
            .request(
                command_set::OBJECT_REFERENCE,
                command::OBJECT_GET_VALUES,
                encoder.finish(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let count = decoder.i32()?.max(0);
        let mut result = Vec::with_capacity(count as usize);
        for _ in 0..count {
            result.push(decoder.tagged_value()?);
        }
        Ok(result)
    }

    async fn resolve_path(
        &self,
        thread: u64,
        frame: u64,
        segments: &[String],
    ) -> Result<Value, DebugError> {
        let Some((first, rest)) = segments.split_first() else {
            return Err(DebugError::Unsupported("empty expression".to_owned()));
        };
        let location = self.frame_location(thread, frame).await?;
        let mut current = if first == "this" {
            match self.this_object(thread, frame).await? {
                Some(id) => Value::Object { tag: b'L', id },
                None => {
                    return Err(DebugError::Unsupported(
                        "static frame has no `this`".to_owned(),
                    ));
                }
            }
        } else {
            let slots = self.variable_slots(location).await?;
            match slots.iter().position(|(name, _, _)| name == first) {
                Some(index) => {
                    let values = self.slot_values(thread, frame, &slots).await?;
                    values
                        .get(index)
                        .cloned()
                        .ok_or_else(|| DebugError::Protocol("missing slot value".to_owned()))?
                }
                None => {
                    // Cai para um campo do objeto corrente, como o Java faria.
                    let Some(this) = self.this_object(thread, frame).await? else {
                        return Err(DebugError::Unsupported(format!("`{first}` is not visible")));
                    };
                    self.field_value(this, first).await?
                }
            }
        };
        for segment in rest {
            let Value::Object { id, .. } = current else {
                return Err(DebugError::Unsupported(format!(
                    "`{segment}` is not a field of a primitive value"
                )));
            };
            if id == 0 {
                return Err(DebugError::Unsupported(format!(
                    "`{segment}` cannot be read from null"
                )));
            }
            current = self.field_value(id, segment).await?;
        }
        Ok(current)
    }

    async fn field_value(&self, object_id: u64, name: &str) -> Result<Value, DebugError> {
        let fields = self.fields_of(object_id).await?;
        let Some(index) = fields.iter().position(|(field, _, _)| field == name) else {
            return Err(DebugError::Unsupported(format!("`{name}` is not visible")));
        };
        let values = self.field_values(object_id, &fields).await?;
        values
            .get(index)
            .cloned()
            .ok_or_else(|| DebugError::Protocol("missing field value".to_owned()))
    }

    // ---------------------------------------------------------------- eventos

    pub(crate) async fn handle_composite(&self, payload: Vec<u8>) {
        if let Err(error) = self.dispatch_composite(payload).await {
            tracing::warn!(%error, "debug event was discarded");
        }
    }

    async fn dispatch_composite(&self, payload: Vec<u8>) -> Result<(), DebugError> {
        let mut decoder = self.connection.decoder(&payload);
        let _suspend_policy = decoder.u8()?;
        let count = decoder.i32()?.max(0);
        for _ in 0..count {
            let kind = decoder.u8()?;
            match kind {
                event_kind::CLASS_PREPARE => {
                    let _request = decoder.i32()?;
                    let thread = decoder.object_id()?;
                    let _tag = decoder.u8()?;
                    let _type_id = decoder.reference_type_id()?;
                    let signature = decoder.string()?;
                    let _status = decoder.i32()?;
                    let installed = self.install_pending(&signature).await;
                    if let Err(error) = installed {
                        tracing::warn!(%error, %signature, "breakpoint installation failed");
                    }
                    // A classe foi carregada com a thread suspensa; liberá-la é
                    // obrigatório para o programa continuar.
                    let _ = self.resume_thread(thread).await;
                }
                event_kind::BREAKPOINT | event_kind::SINGLE_STEP => {
                    let request = decoder.i32()?;
                    let thread = decoder.object_id()?;
                    let _location = decoder.location()?;
                    let reason = {
                        let mut state = self.state.lock().await;
                        state.suspended.insert(thread);
                        if state.step_requests.remove(&request) {
                            StopReason::Step
                        } else {
                            state
                                .breakpoint_requests
                                .get(&request)
                                .copied()
                                .map_or(StopReason::Step, StopReason::Breakpoint)
                        }
                    };
                    if kind == event_kind::SINGLE_STEP {
                        self.clear_request(event_kind::SINGLE_STEP, request).await;
                    }
                    self.cache.lock().await.frames.clear();
                    self.events.emit(DebugEvent::Stopped {
                        thread: ThreadId(thread),
                        reason,
                    });
                }
                event_kind::EXCEPTION => {
                    let _request = decoder.i32()?;
                    let thread = decoder.object_id()?;
                    let _location = decoder.location()?;
                    let exception = decoder.tagged_value()?;
                    let _catch = decoder.location()?;
                    let (text, _, _) = self.describe(&exception).await;
                    self.state.lock().await.suspended.insert(thread);
                    self.cache.lock().await.frames.clear();
                    self.events.emit(DebugEvent::Stopped {
                        thread: ThreadId(thread),
                        reason: StopReason::Exception(text),
                    });
                }
                event_kind::VM_DEATH => {
                    let _request = decoder.i32()?;
                    self.state.lock().await.detached = true;
                    self.events.emit(DebugEvent::Detached {
                        reason: Some("o processo depurado terminou".to_owned()),
                    });
                }
                event_kind::VM_START | event_kind::THREAD_START | event_kind::THREAD_DEATH => {
                    let _request = decoder.i32()?;
                    let _thread = decoder.object_id()?;
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    pub(crate) fn notify_detached(&self, reason: Option<String>) {
        self.events.emit(DebugEvent::Detached { reason });
    }

    async fn resume_thread(&self, thread: u64) -> Result<(), DebugError> {
        let mut encoder = self.connection.encoder();
        encoder.object_id(thread);
        self.connection
            .request(
                command_set::THREAD_REFERENCE,
                command::THREAD_RESUME,
                encoder.finish(),
            )
            .await?;
        Ok(())
    }

    async fn ensure_attached(&self) -> Result<(), DebugError> {
        if self.state.lock().await.detached {
            return Err(DebugError::Detached);
        }
        Ok(())
    }
}

#[async_trait]
impl DebugSession for Session {
    async fn set_breakpoints(
        &self,
        path: &Path,
        breakpoints: &[SourceBreakpoint],
    ) -> Result<Vec<ResolvedBreakpoint>, DebugError> {
        self.ensure_attached().await?;
        let previous = self
            .state
            .lock()
            .await
            .files
            .remove(path)
            .unwrap_or_default();
        for breakpoint in &previous {
            for request in &breakpoint.requests {
                self.clear_request(event_kind::BREAKPOINT, *request).await;
                self.state.lock().await.breakpoint_requests.remove(request);
            }
        }
        if breakpoints.is_empty() {
            return Ok(Vec::new());
        }

        let unmapped = |message: &str| -> Vec<ResolvedBreakpoint> {
            breakpoints
                .iter()
                .map(|breakpoint| ResolvedBreakpoint {
                    id: BreakpointId(0),
                    requested: breakpoint.clone(),
                    verified_line: None,
                    message: Some(message.to_owned()),
                })
                .collect()
        };
        let Some(relative) = resolve::relative_source(path, &self.source_roots) else {
            return Ok(unmapped("arquivo fora das raízes de código do projeto"));
        };
        let Some(fully_qualified) = resolve::fully_qualified_name(&relative) else {
            return Ok(unmapped("arquivo sem classe correspondente"));
        };

        self.watch_class(&resolve::class_match_pattern(&fully_qualified))
            .await?;
        let classes = self
            .classes_matching(&resolve::signature_prefix(&fully_qualified))
            .await?;

        let mut installed = Vec::new();
        for breakpoint in breakpoints {
            let (requests, verified_line, message) =
                self.install_in_classes(&classes, breakpoint.line).await?;
            let mut state = self.state.lock().await;
            state.next_breakpoint += 1;
            let id = BreakpointId(state.next_breakpoint);
            for request in &requests {
                state.breakpoint_requests.insert(*request, id);
            }
            installed.push(InstalledBreakpoint {
                id,
                requested: breakpoint.clone(),
                verified_line,
                message,
                requests,
            });
        }
        let resolved = installed
            .iter()
            .map(InstalledBreakpoint::resolved)
            .collect();
        self.state
            .lock()
            .await
            .files
            .insert(path.to_path_buf(), installed);
        Ok(resolved)
    }

    async fn threads(&self) -> Result<Vec<ThreadDescriptor>, DebugError> {
        self.ensure_attached().await?;
        let payload = self
            .connection
            .request(
                command_set::VIRTUAL_MACHINE,
                command::VM_ALL_THREADS,
                Vec::new(),
            )
            .await?;
        let mut decoder = self.connection.decoder(&payload);
        let count = decoder.i32()?.max(0);
        let mut ids = Vec::with_capacity(count as usize);
        for _ in 0..count {
            ids.push(decoder.object_id()?);
        }
        let mut threads = Vec::with_capacity(ids.len());
        for id in ids {
            let mut encoder = self.connection.encoder();
            encoder.object_id(id);
            let name = match self
                .connection
                .request(
                    command_set::THREAD_REFERENCE,
                    command::THREAD_NAME,
                    encoder.finish(),
                )
                .await
            {
                Ok(payload) => self.connection.decoder(&payload).string()?,
                Err(_) => continue,
            };
            let mut encoder = self.connection.encoder();
            encoder.object_id(id);
            let suspended = match self
                .connection
                .request(
                    command_set::THREAD_REFERENCE,
                    command::THREAD_STATUS,
                    encoder.finish(),
                )
                .await
            {
                Ok(payload) => {
                    let mut decoder = self.connection.decoder(&payload);
                    let _status = decoder.i32()?;
                    decoder.i32()? & 1 != 0
                }
                Err(_) => self.state.lock().await.suspended.contains(&id),
            };
            threads.push(ThreadDescriptor {
                id: ThreadId(id),
                name,
                suspended,
            });
        }
        Ok(threads)
    }

    async fn stack_trace(&self, thread: ThreadId) -> Result<Vec<StackFrame>, DebugError> {
        self.ensure_attached().await?;
        let frames = self.frames(thread.0).await?;
        let mut result = Vec::with_capacity(frames.len());
        for (id, location) in frames {
            let signature = self.signature(location.class_id).await.unwrap_or_default();
            let method = self
                .methods(location.class_id)
                .await
                .ok()
                .and_then(|methods| {
                    methods
                        .iter()
                        .find(|method| method.id == location.method_id)
                        .map(|method| method.name.clone())
                })
                .unwrap_or_else(|| "?".to_owned());
            result.push(StackFrame {
                id: FrameId(id),
                name: format!("{}.{method}", type_name(&signature)),
                location: self.location_of(location).await,
            });
        }
        Ok(result)
    }

    async fn variables(
        &self,
        thread: ThreadId,
        frame: FrameId,
    ) -> Result<Vec<Variable>, DebugError> {
        self.ensure_attached().await?;
        let location = self.frame_location(thread.0, frame.0).await?;
        let slots = self.variable_slots(location).await?;
        let values = self.slot_values(thread.0, frame.0, &slots).await?;
        let mut variables = Vec::new();
        if let Some(id) = self.this_object(thread.0, frame.0).await? {
            variables.push(self.present("this", &Value::Object { tag: b'L', id }).await);
        }
        for ((name, _, _), value) in slots.iter().zip(values.iter()) {
            variables.push(self.present(name, value).await);
        }
        Ok(variables)
    }

    async fn expand(
        &self,
        thread: ThreadId,
        frame: FrameId,
        path: &str,
    ) -> Result<Vec<Variable>, DebugError> {
        self.ensure_attached().await?;
        let Some(segments) = values::parse_path(path) else {
            return Err(DebugError::Unsupported(format!(
                "`{path}` is not an inspection path"
            )));
        };
        let value = self.resolve_path(thread.0, frame.0, &segments).await?;
        let Value::Object { id, .. } = value else {
            return Ok(Vec::new());
        };
        if id == 0 {
            return Ok(Vec::new());
        }
        let fields = self.fields_of(id).await?;
        let field_values = self.field_values(id, &fields).await?;
        let mut variables = Vec::with_capacity(fields.len());
        for ((name, _, _), value) in fields.iter().zip(field_values.iter()) {
            variables.push(self.present(name, value).await);
        }
        Ok(variables)
    }

    async fn evaluate(
        &self,
        thread: ThreadId,
        frame: FrameId,
        expression: &str,
    ) -> Result<Variable, DebugError> {
        self.ensure_attached().await?;
        // Uma chamada executa no alvo; um caminho apenas lê. A expressão diz qual
        // das duas coisas o usuário pediu.
        if let Some(call) = values::parse_call(expression) {
            let value = self.invoke(thread.0, frame.0, &call).await?;
            return Ok(self.present(expression, &value).await);
        }
        let Some(segments) = values::parse_path(expression) else {
            return Err(DebugError::Unsupported(
                "escreva um caminho — `pedido.cliente.nome` — ou uma chamada de método".to_owned(),
            ));
        };
        let value = self.resolve_path(thread.0, frame.0, &segments).await?;
        Ok(self.present(expression, &value).await)
    }

    async fn step(&self, thread: ThreadId, kind: StepKind) -> Result<(), DebugError> {
        self.ensure_attached().await?;
        let depth = match kind {
            StepKind::Into => step::DEPTH_INTO,
            StepKind::Over => step::DEPTH_OVER,
            StepKind::Out => step::DEPTH_OUT,
        };
        let mut encoder = self.connection.encoder();
        encoder
            .u8(event_kind::SINGLE_STEP)
            .u8(suspend_policy::EVENT_THREAD)
            .i32(1)
            .u8(modifier::STEP)
            .object_id(thread.0);
        encoder.i32(step::SIZE_LINE).i32(depth);
        let payload = self
            .connection
            .request(
                command_set::EVENT_REQUEST,
                command::EVENT_REQUEST_SET,
                encoder.finish(),
            )
            .await?;
        let request = self.connection.decoder(&payload).i32()?;
        self.state.lock().await.step_requests.insert(request);
        self.resume(Some(thread)).await
    }

    async fn resume(&self, thread: Option<ThreadId>) -> Result<(), DebugError> {
        self.ensure_attached().await?;
        let targets: Vec<u64> = match thread {
            Some(thread) => vec![thread.0],
            None => self.state.lock().await.suspended.iter().copied().collect(),
        };
        if targets.is_empty() {
            self.connection
                .request(command_set::VIRTUAL_MACHINE, command::VM_RESUME, Vec::new())
                .await?;
            return Ok(());
        }
        for target in targets {
            self.resume_thread(target).await?;
            self.state.lock().await.suspended.remove(&target);
            self.cache.lock().await.frames.clear();
            self.events.emit(DebugEvent::Resumed {
                thread: ThreadId(target),
            });
        }
        Ok(())
    }

    async fn pause(&self, thread: ThreadId) -> Result<(), DebugError> {
        self.ensure_attached().await?;
        let mut encoder = self.connection.encoder();
        encoder.object_id(thread.0);
        self.connection
            .request(
                command_set::THREAD_REFERENCE,
                command::THREAD_SUSPEND,
                encoder.finish(),
            )
            .await?;
        self.state.lock().await.suspended.insert(thread.0);
        self.cache.lock().await.frames.clear();
        self.events.emit(DebugEvent::Stopped {
            thread,
            reason: StopReason::Pause,
        });
        Ok(())
    }

    async fn detach(&self) -> Result<(), DebugError> {
        {
            let mut state = self.state.lock().await;
            if state.detached {
                return Ok(());
            }
            state.detached = true;
        }
        self.connection.dispose().await;
        self.events.emit(DebugEvent::Detached { reason: None });
        Ok(())
    }
}

/// Quantidade de argumentos de uma assinatura JNI.
fn signature_arity(signature: &str) -> usize {
    let Some(inside) = signature
        .strip_prefix('(')
        .and_then(|rest| rest.split(')').next())
    else {
        return 0;
    };
    let mut count = 0;
    let mut rest = inside;
    while !rest.is_empty() {
        let arrays = rest.len() - rest.trim_start_matches('[').len();
        let body = &rest[arrays..];
        let consumed = if body.starts_with('L') {
            body.find(';').map_or(body.len(), |index| index + 1)
        } else {
            1
        };
        count += 1;
        rest = &rest[arrays + consumed..];
    }
    count
}

/// Escreve um literal como valor etiquetado do protocolo.
fn encode_literal(encoder: &mut Encoder, literal: &values::Literal) {
    match literal {
        values::Literal::Null | values::Literal::Text(_) => {
            // Texto exigiria criar uma `String` no alvo por
            // `VirtualMachine.CreateString`, que é outra ida ao processo.
            encoder.u8(b'L');
            encoder.object_id(0);
        }
        values::Literal::Bool(value) => {
            encoder.u8(b'Z');
            encoder.u8(u8::from(*value));
        }
        values::Literal::Int(value) => {
            encoder.u8(b'I');
            encoder.i32(*value);
        }
        values::Literal::Long(value) => {
            encoder.u8(b'J');
            encoder.i64(*value);
        }
        values::Literal::Double(value) => {
            encoder.u8(b'D');
            encoder.i64(value.to_bits() as i64);
        }
    }
}
