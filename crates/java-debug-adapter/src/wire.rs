//! Codificação binária do protocolo de depuração da JVM.
//!
//! Todos os inteiros são big-endian. Os identificadores têm largura variável,
//! informada pelo alvo em `VirtualMachine.IDSizes`, e por isso codificador e
//! decodificador carregam essas larguras.

use ide_debug_api::DebugError;

pub(crate) const HANDSHAKE: &[u8] = b"JDWP-Handshake";
pub(crate) const HEADER_LEN: usize = 11;
pub(crate) const FLAG_REPLY: u8 = 0x80;

pub(crate) mod command_set {
    pub(crate) const VIRTUAL_MACHINE: u8 = 1;
    pub(crate) const REFERENCE_TYPE: u8 = 2;
    pub(crate) const CLASS_TYPE: u8 = 3;
    pub(crate) const METHOD: u8 = 6;
    pub(crate) const OBJECT_REFERENCE: u8 = 9;
    pub(crate) const STRING_REFERENCE: u8 = 10;
    pub(crate) const THREAD_REFERENCE: u8 = 11;
    pub(crate) const ARRAY_REFERENCE: u8 = 13;
    pub(crate) const EVENT_REQUEST: u8 = 15;
    pub(crate) const STACK_FRAME: u8 = 16;
    pub(crate) const EVENT: u8 = 64;
}

pub(crate) mod command {
    pub(crate) const VM_VERSION: u8 = 1;
    pub(crate) const VM_CLASSES_BY_SIGNATURE: u8 = 2;
    pub(crate) const VM_ALL_CLASSES: u8 = 3;
    pub(crate) const VM_ALL_THREADS: u8 = 4;
    pub(crate) const VM_DISPOSE: u8 = 6;
    pub(crate) const VM_ID_SIZES: u8 = 7;
    pub(crate) const VM_RESUME: u8 = 9;
    pub(crate) const VM_CREATE_STRING: u8 = 11;

    pub(crate) const REFERENCE_TYPE_FIELDS: u8 = 4;
    pub(crate) const REFERENCE_TYPE_METHODS: u8 = 5;
    pub(crate) const REFERENCE_TYPE_SIGNATURE: u8 = 1;

    pub(crate) const CLASS_TYPE_SUPERCLASS: u8 = 1;
    pub(crate) const CLASS_TYPE_INVOKE_METHOD: u8 = 3;

    pub(crate) const METHOD_LINE_TABLE: u8 = 1;
    pub(crate) const METHOD_VARIABLE_TABLE: u8 = 2;

    pub(crate) const OBJECT_REFERENCE_TYPE: u8 = 1;
    pub(crate) const OBJECT_GET_VALUES: u8 = 2;
    pub(crate) const OBJECT_INVOKE_METHOD: u8 = 6;

    pub(crate) const STRING_VALUE: u8 = 1;

    pub(crate) const THREAD_NAME: u8 = 1;
    pub(crate) const THREAD_SUSPEND: u8 = 2;
    pub(crate) const THREAD_RESUME: u8 = 3;
    pub(crate) const THREAD_STATUS: u8 = 4;
    pub(crate) const THREAD_FRAMES: u8 = 6;

    pub(crate) const ARRAY_LENGTH: u8 = 1;

    pub(crate) const EVENT_REQUEST_SET: u8 = 1;
    pub(crate) const EVENT_REQUEST_CLEAR: u8 = 2;

    pub(crate) const STACK_FRAME_GET_VALUES: u8 = 1;
    pub(crate) const STACK_FRAME_THIS_OBJECT: u8 = 3;

    pub(crate) const EVENT_COMPOSITE: u8 = 100;
}

/// Opções de `InvokeMethod`, pela especificação do JDWP.
pub(crate) mod invoke {
    /// Só a thread escolhida roda durante a chamada.
    ///
    /// Retomar a VM inteira para executar um método faria o resto do programa
    /// avançar enquanto o usuário inspeciona — e o estado que ele está olhando
    /// deixaria de ser o estado em que a execução parou.
    pub(crate) const SINGLE_THREADED: i32 = 0x01;
}

pub(crate) mod event_kind {
    pub(crate) const SINGLE_STEP: u8 = 1;
    pub(crate) const BREAKPOINT: u8 = 2;
    pub(crate) const EXCEPTION: u8 = 4;
    pub(crate) const THREAD_START: u8 = 6;
    pub(crate) const THREAD_DEATH: u8 = 7;
    pub(crate) const CLASS_PREPARE: u8 = 8;
    pub(crate) const VM_START: u8 = 90;
    pub(crate) const VM_DEATH: u8 = 99;
}

pub(crate) mod modifier {
    pub(crate) const CLASS_MATCH: u8 = 5;
    pub(crate) const LOCATION_ONLY: u8 = 7;
    pub(crate) const STEP: u8 = 10;
}

pub(crate) mod suspend_policy {
    /// Só a thread do evento para; as demais seguem executando.
    pub(crate) const EVENT_THREAD: u8 = 1;
}

pub(crate) mod step {
    pub(crate) const SIZE_LINE: i32 = 1;
    pub(crate) const DEPTH_INTO: i32 = 0;
    pub(crate) const DEPTH_OVER: i32 = 1;
    pub(crate) const DEPTH_OUT: i32 = 2;
}

/// Larguras dos identificadores, informadas pelo alvo na conexão.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IdSizes {
    pub(crate) field: usize,
    pub(crate) method: usize,
    pub(crate) object: usize,
    pub(crate) reference_type: usize,
    pub(crate) frame: usize,
}

impl Default for IdSizes {
    fn default() -> Self {
        Self {
            field: 8,
            method: 8,
            object: 8,
            reference_type: 8,
            frame: 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct JdwpLocation {
    pub(crate) tag: u8,
    pub(crate) class_id: u64,
    pub(crate) method_id: u64,
    pub(crate) index: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Bool(bool),
    Byte(i8),
    Char(u16),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Void,
    /// `id` zero significa `null`.
    Object {
        tag: u8,
        id: u64,
    },
}

pub(crate) struct Encoder {
    data: Vec<u8>,
    sizes: IdSizes,
}

impl Encoder {
    pub(crate) fn new(sizes: IdSizes) -> Self {
        Self {
            data: Vec::new(),
            sizes,
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.data
    }

    pub(crate) fn u8(&mut self, value: u8) -> &mut Self {
        self.data.push(value);
        self
    }

    pub(crate) fn i32(&mut self, value: i32) -> &mut Self {
        self.data.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub(crate) fn i64(&mut self, value: i64) -> &mut Self {
        self.data.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub(crate) fn string(&mut self, value: &str) -> &mut Self {
        self.i32(value.len() as i32);
        self.data.extend_from_slice(value.as_bytes());
        self
    }

    fn id(&mut self, value: u64, size: usize) -> &mut Self {
        let bytes = value.to_be_bytes();
        let start = bytes.len().saturating_sub(size);
        self.data.extend_from_slice(&bytes[start..]);
        self
    }

    pub(crate) fn object_id(&mut self, value: u64) -> &mut Self {
        let size = self.sizes.object;
        self.id(value, size)
    }

    pub(crate) fn reference_type_id(&mut self, value: u64) -> &mut Self {
        let size = self.sizes.reference_type;
        self.id(value, size)
    }

    pub(crate) fn method_id(&mut self, value: u64) -> &mut Self {
        let size = self.sizes.method;
        self.id(value, size)
    }

    pub(crate) fn field_id(&mut self, value: u64) -> &mut Self {
        let size = self.sizes.field;
        self.id(value, size)
    }

    pub(crate) fn frame_id(&mut self, value: u64) -> &mut Self {
        let size = self.sizes.frame;
        self.id(value, size)
    }

    /// Escreve um valor precedido da etiqueta do seu tipo.
    ///
    /// A etiqueta é o que o alvo usa para saber o que veio, e ele **não** a
    /// confere contra o tipo declarado: um primitivo enviado onde se espera uma
    /// referência é lido como endereço de objeto e derruba o processo depurado.
    /// Por isso quem monta argumentos precisa acertar o tipo antes de chegar
    /// aqui.
    pub(crate) fn tagged_value(&mut self, value: &Value) -> &mut Self {
        match value {
            Value::Bool(value) => self.u8(b'Z').u8(u8::from(*value)),
            Value::Byte(value) => self.u8(b'B').u8(*value as u8),
            Value::Char(value) => self
                .u8(b'C')
                .u8((*value >> 8) as u8)
                .u8((*value & 0xff) as u8),
            Value::Short(value) => self
                .u8(b'S')
                .u8((*value >> 8) as u8)
                .u8((*value & 0xff) as u8),
            Value::Int(value) => self.u8(b'I').i32(*value),
            Value::Long(value) => self.u8(b'J').i64(*value),
            Value::Float(value) => self.u8(b'F').i32(value.to_bits() as i32),
            Value::Double(value) => self.u8(b'D').i64(value.to_bits() as i64),
            Value::Void => self.u8(b'V'),
            Value::Object { tag, id } => {
                self.u8(*tag);
                self.object_id(*id)
            }
        }
    }

    pub(crate) fn location(&mut self, location: JdwpLocation) -> &mut Self {
        self.u8(location.tag);
        self.reference_type_id(location.class_id);
        self.method_id(location.method_id);
        self.i64(location.index as i64)
    }
}

pub(crate) struct Decoder<'a> {
    data: &'a [u8],
    offset: usize,
    sizes: IdSizes,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(data: &'a [u8], sizes: IdSizes) -> Self {
        Self {
            data,
            offset: 0,
            sizes,
        }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], DebugError> {
        let end = self.offset.checked_add(count).ok_or_else(overflow)?;
        let slice = self.data.get(self.offset..end).ok_or_else(|| {
            DebugError::Protocol(format!(
                "reply is truncated: expected {count} bytes at offset {}",
                self.offset
            ))
        })?;
        self.offset = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, DebugError> {
        self.take(1)?.first().copied().ok_or_else(truncated)
    }

    pub(crate) fn i32(&mut self) -> Result<i32, DebugError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| truncated())?;
        Ok(i32::from_be_bytes(bytes))
    }

    pub(crate) fn i64(&mut self) -> Result<i64, DebugError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| truncated())?;
        Ok(i64::from_be_bytes(bytes))
    }

    pub(crate) fn string(&mut self) -> Result<String, DebugError> {
        let length = self.i32()?.max(0) as usize;
        let bytes = self.take(length)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn id(&mut self, size: usize) -> Result<u64, DebugError> {
        let bytes = self.take(size)?;
        Ok(bytes
            .iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte)))
    }

    pub(crate) fn object_id(&mut self) -> Result<u64, DebugError> {
        self.id(self.sizes.object)
    }

    pub(crate) fn reference_type_id(&mut self) -> Result<u64, DebugError> {
        self.id(self.sizes.reference_type)
    }

    pub(crate) fn method_id(&mut self) -> Result<u64, DebugError> {
        self.id(self.sizes.method)
    }

    pub(crate) fn field_id(&mut self) -> Result<u64, DebugError> {
        self.id(self.sizes.field)
    }

    pub(crate) fn frame_id(&mut self) -> Result<u64, DebugError> {
        self.id(self.sizes.frame)
    }

    pub(crate) fn location(&mut self) -> Result<JdwpLocation, DebugError> {
        Ok(JdwpLocation {
            tag: self.u8()?,
            class_id: self.reference_type_id()?,
            method_id: self.method_id()?,
            index: self.i64()? as u64,
        })
    }

    pub(crate) fn tagged_value(&mut self) -> Result<Value, DebugError> {
        let tag = self.u8()?;
        self.value(tag)
    }

    pub(crate) fn value(&mut self, tag: u8) -> Result<Value, DebugError> {
        Ok(match tag {
            b'Z' => Value::Bool(self.u8()? != 0),
            b'B' => Value::Byte(self.u8()? as i8),
            b'C' => Value::Char(u16::from(self.u8()?) << 8 | u16::from(self.u8()?)),
            b'S' => Value::Short(self.i32_from(2)? as i16),
            b'I' => Value::Int(self.i32()?),
            b'J' => Value::Long(self.i64()?),
            b'F' => Value::Float(f32::from_bits(self.i32()? as u32)),
            b'D' => Value::Double(f64::from_bits(self.i64()? as u64)),
            b'V' => Value::Void,
            _ => Value::Object {
                tag,
                id: self.object_id()?,
            },
        })
    }

    fn i32_from(&mut self, bytes: usize) -> Result<i32, DebugError> {
        let slice = self.take(bytes)?;
        Ok(slice
            .iter()
            .fold(0_i32, |value, byte| (value << 8) | i32::from(*byte)))
    }
}

fn truncated() -> DebugError {
    DebugError::Protocol("reply is truncated".to_owned())
}

fn overflow() -> DebugError {
    DebugError::Protocol("reply length overflowed".to_owned())
}

/// Cabeçalho de 11 bytes comum a comandos e respostas.
pub(crate) fn encode_command(id: u32, command_set: u8, command: u8, payload: &[u8]) -> Vec<u8> {
    let length = (HEADER_LEN + payload.len()) as u32;
    let mut packet = Vec::with_capacity(length as usize);
    packet.extend_from_slice(&length.to_be_bytes());
    packet.extend_from_slice(&id.to_be_bytes());
    packet.push(0);
    packet.push(command_set);
    packet.push(command);
    packet.extend_from_slice(payload);
    packet
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PacketHeader {
    pub(crate) length: u32,
    pub(crate) id: u32,
    pub(crate) flags: u8,
    /// Código de erro nas respostas, `command_set`/`command` nos comandos.
    pub(crate) error_code: u16,
    pub(crate) command_set: u8,
    pub(crate) command: u8,
}

impl PacketHeader {
    pub(crate) const fn is_reply(&self) -> bool {
        self.flags & FLAG_REPLY != 0
    }
}

pub(crate) fn decode_header(bytes: &[u8; HEADER_LEN]) -> PacketHeader {
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let id = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    PacketHeader {
        length,
        id,
        flags: bytes[8],
        error_code: u16::from_be_bytes([bytes[9], bytes[10]]),
        command_set: bytes[9],
        command: bytes[10],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trips_through_the_header() {
        let mut encoder = Encoder::new(IdSizes::default());
        encoder.string("Lcom/example/Main;");
        let packet = encode_command(
            7,
            command_set::VIRTUAL_MACHINE,
            command::VM_ALL_CLASSES,
            &encoder.finish(),
        );

        let header_bytes: [u8; HEADER_LEN] = match packet[..HEADER_LEN].try_into() {
            Ok(bytes) => bytes,
            Err(error) => panic!("header slice failed: {error}"),
        };
        let header = decode_header(&header_bytes);
        assert_eq!(header.length as usize, packet.len());
        assert_eq!(header.id, 7);
        assert!(!header.is_reply());
        assert_eq!(header.command_set, command_set::VIRTUAL_MACHINE);
        assert_eq!(header.command, command::VM_ALL_CLASSES);

        let mut decoder = Decoder::new(&packet[HEADER_LEN..], IdSizes::default());
        assert_eq!(decoder.string().ok().as_deref(), Some("Lcom/example/Main;"));
    }

    #[test]
    fn identifiers_use_the_width_reported_by_the_target() {
        let sizes = IdSizes {
            field: 4,
            method: 4,
            object: 4,
            reference_type: 4,
            frame: 4,
        };
        let mut encoder = Encoder::new(sizes);
        encoder.object_id(0x0102_0304).method_id(9);
        let payload = encoder.finish();
        assert_eq!(payload.len(), 8, "dois identificadores de quatro bytes");

        let mut decoder = Decoder::new(&payload, sizes);
        assert_eq!(decoder.object_id().ok(), Some(0x0102_0304));
        assert_eq!(decoder.method_id().ok(), Some(9));
    }

    #[test]
    fn tagged_values_decode_each_primitive_and_object_form() {
        let sizes = IdSizes::default();
        let mut encoder = Encoder::new(sizes);
        encoder.u8(b'I').i32(-5);
        encoder.u8(b'Z').u8(1);
        encoder.u8(b'J').i64(1 << 40);
        encoder.u8(b'L').object_id(0);
        encoder.u8(b's').object_id(42);
        let payload = encoder.finish();

        let mut decoder = Decoder::new(&payload, sizes);
        assert_eq!(decoder.tagged_value().ok(), Some(Value::Int(-5)));
        assert_eq!(decoder.tagged_value().ok(), Some(Value::Bool(true)));
        assert_eq!(decoder.tagged_value().ok(), Some(Value::Long(1 << 40)));
        assert_eq!(
            decoder.tagged_value().ok(),
            Some(Value::Object { tag: b'L', id: 0 })
        );
        assert_eq!(
            decoder.tagged_value().ok(),
            Some(Value::Object { tag: b's', id: 42 })
        );
    }

    #[test]
    fn truncated_replies_produce_a_protocol_error_instead_of_panicking() {
        let mut decoder = Decoder::new(&[0, 0], IdSizes::default());
        assert!(matches!(decoder.i32(), Err(DebugError::Protocol(_))));
    }
}
