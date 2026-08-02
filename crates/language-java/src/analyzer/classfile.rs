#![doc = "Leitor limitado e seguro de metadados de class files e arquivos JAR."]

use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};

use thiserror::Error;

const CLASS_MAGIC: u32 = 0xCAFE_BABE;
const MAX_CLASS_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassMember {
    pub name: String,
    pub descriptor: String,
    pub access_flags: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassDescriptor {
    pub binary_name: String,
    pub super_name: Option<String>,
    pub access_flags: u16,
    pub major_version: u16,
    pub fields: Vec<ClassMember>,
    pub methods: Vec<ClassMember>,
}

#[derive(Debug, Error)]
pub enum ClassFileError {
    #[error("invalid or truncated class file")]
    Invalid,
    #[error("unsupported constant pool tag {0}")]
    UnsupportedConstant(u8),
    #[error("class or JAR I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JAR: {0}")]
    Jar(#[from] zip::result::ZipError),
}

#[derive(Clone, Debug)]
enum Constant {
    Empty,
    Utf8(String),
    Class(u16),
}

pub fn read_class(bytes: &[u8]) -> Result<ClassDescriptor, ClassFileError> {
    let mut reader = ByteReader::new(bytes);
    if reader.u4()? != CLASS_MAGIC {
        return Err(ClassFileError::Invalid);
    }
    let _minor = reader.u2()?;
    let major_version = reader.u2()?;
    let constant_count = reader.u2()? as usize;
    if constant_count == 0 {
        return Err(ClassFileError::Invalid);
    }
    let mut constants = vec![Constant::Empty];
    let mut index = 1;
    while index < constant_count {
        let tag = reader.u1()?;
        match tag {
            1 => {
                let length = reader.u2()? as usize;
                let text = String::from_utf8_lossy(reader.take(length)?).into_owned();
                constants.push(Constant::Utf8(text));
            }
            7 => constants.push(Constant::Class(reader.u2()?)),
            3 | 4 => {
                reader.skip(4)?;
                constants.push(Constant::Empty);
            }
            5 | 6 => {
                reader.skip(8)?;
                constants.push(Constant::Empty);
                constants.push(Constant::Empty);
                index += 1;
            }
            8 | 16 | 19 | 20 => {
                reader.skip(2)?;
                constants.push(Constant::Empty);
            }
            9 | 10 | 11 | 12 | 17 | 18 => {
                reader.skip(4)?;
                constants.push(Constant::Empty);
            }
            15 => {
                reader.skip(3)?;
                constants.push(Constant::Empty);
            }
            other => return Err(ClassFileError::UnsupportedConstant(other)),
        }
        index += 1;
    }
    if constants.len() != constant_count {
        return Err(ClassFileError::Invalid);
    }

    let access_flags = reader.u2()?;
    let this_class = reader.u2()?;
    let super_class = reader.u2()?;
    let interface_count = reader.u2()?;
    reader.skip(interface_count as usize * 2)?;
    let fields = read_members(&mut reader, &constants)?;
    let methods = read_members(&mut reader, &constants)?;
    skip_attributes(&mut reader)?;

    Ok(ClassDescriptor {
        binary_name: class_name(&constants, this_class)?.replace('/', "."),
        super_name: if super_class == 0 {
            None
        } else {
            Some(class_name(&constants, super_class)?.replace('/', "."))
        },
        access_flags,
        major_version,
        fields,
        methods,
    })
}

pub fn index_jar(path: &Path, max_classes: usize) -> Result<Vec<ClassDescriptor>, ClassFileError> {
    index_jar_reader(File::open(path)?, max_classes)
}

/// Nomes binários das classes de um arquivo, lidos só pelo diretório dele.
///
/// O nome de uma classe está no caminho da entrada, e o caminho está no
/// diretório central do zip. Abrir e decodificar cada `.class` só para descobrir
/// como ele se chama custa milhares de leituras — em `java.base.jmod` são mais
/// de seis mil — e o índice precisa apenas do nome. Os membros vêm depois, de
/// uma classe por vez, em [`read_class_in_archive`].
///
/// Serve jar, zip e `.jmod`, que guarda as classes sob `classes/`.
pub fn list_classes(path: &Path, max_classes: usize) -> Result<Vec<String>, ClassFileError> {
    let archive = zip::ZipArchive::new(File::open(path)?)?;
    Ok(archive
        .file_names()
        .filter_map(binary_name_of_entry)
        .take(max_classes)
        .collect())
}

/// Nome binário de uma entrada de arquivo compactado, se ela for uma classe.
fn binary_name_of_entry(entry: &str) -> Option<String> {
    let class = entry.strip_suffix(".class")?;
    // Um `.jmod` prefixa tudo com `classes/`; um jar não prefixa nada.
    let class = class.strip_prefix("classes/").unwrap_or(class);
    // Classe anônima — `System$1` — não tem nome que alguém escreva, e um
    // arquivo de metadados de pacote ou módulo não é um tipo.
    let anonymous = class
        .rsplit('$')
        .next()
        .is_some_and(|last| !last.is_empty() && last.chars().all(|value| value.is_ascii_digit()));
    (!anonymous && !class.ends_with("package-info") && !class.ends_with("module-info"))
        .then(|| class.replace('/', "."))
}

/// Lê uma única classe de dentro de um jar, zip ou `.jmod`, pelo nome binário.
///
/// Indexar o arquivo inteiro para responder sobre um tipo custa milhares de
/// leituras que ninguém pediu. Quem já sabe o nome — porque ele veio do índice —
/// abre só a entrada que interessa.
pub fn read_class_in_archive(
    path: &Path,
    binary_name: &str,
) -> Result<ClassDescriptor, ClassFileError> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let relative = format!("{}.class", binary_name.replace('.', "/"));
    // Um `.jmod` prefixa tudo com `classes/`; um jar não prefixa nada.
    let entry_name = if archive.index_for_name(&relative).is_some() {
        relative
    } else {
        format!("classes/{relative}")
    };
    let mut entry = archive.by_name(&entry_name)?;
    if entry.size() > MAX_CLASS_BYTES {
        return Err(ClassFileError::Invalid);
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    read_class(&bytes)
}

pub fn index_jar_reader<R: Read + Seek>(
    reader: R,
    max_classes: usize,
) -> Result<Vec<ClassDescriptor>, ClassFileError> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut classes = Vec::new();
    for index in 0..archive.len() {
        if classes.len() >= max_classes {
            break;
        }
        let mut entry = archive.by_index(index)?;
        if !entry.name().ends_with(".class") || entry.size() > MAX_CLASS_BYTES {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        if let Ok(class) = read_class(&bytes) {
            classes.push(class);
        }
    }
    Ok(classes)
}

fn read_members(
    reader: &mut ByteReader<'_>,
    constants: &[Constant],
) -> Result<Vec<ClassMember>, ClassFileError> {
    let count = reader.u2()? as usize;
    let mut members = Vec::with_capacity(count);
    for _ in 0..count {
        let access_flags = reader.u2()?;
        let name = utf8(constants, reader.u2()?)?.to_owned();
        let descriptor = utf8(constants, reader.u2()?)?.to_owned();
        skip_attributes(reader)?;
        members.push(ClassMember {
            name,
            descriptor,
            access_flags,
        });
    }
    Ok(members)
}

fn skip_attributes(reader: &mut ByteReader<'_>) -> Result<(), ClassFileError> {
    let count = reader.u2()? as usize;
    for _ in 0..count {
        reader.skip(2)?;
        let length = reader.u4()? as usize;
        reader.skip(length)?;
    }
    Ok(())
}

fn class_name(constants: &[Constant], index: u16) -> Result<String, ClassFileError> {
    match constants.get(index as usize) {
        Some(Constant::Class(name_index)) => Ok(utf8(constants, *name_index)?.to_owned()),
        _ => Err(ClassFileError::Invalid),
    }
}

fn utf8(constants: &[Constant], index: u16) -> Result<&str, ClassFileError> {
    match constants.get(index as usize) {
        Some(Constant::Utf8(value)) => Ok(value),
        _ => Err(ClassFileError::Invalid),
    }
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn u1(&mut self) -> Result<u8, ClassFileError> {
        Ok(self.take(1)?[0])
    }

    fn u2(&mut self) -> Result<u16, ClassFileError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u4(&mut self) -> Result<u32, ClassFileError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn skip(&mut self, length: usize) -> Result<(), ClassFileError> {
        let _ = self.take(length)?;
        Ok(())
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ClassFileError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(ClassFileError::Invalid)?;
        let result = self
            .bytes
            .get(self.position..end)
            .ok_or(ClassFileError::Invalid)?;
        self.position = end;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn minimal_class() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&CLASS_MAGIC.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&52_u16.to_be_bytes());
        bytes.extend_from_slice(&5_u16.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&8_u16.to_be_bytes());
        bytes.extend_from_slice(b"pkg/Test");
        bytes.push(7);
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&16_u16.to_be_bytes());
        bytes.extend_from_slice(b"java/lang/Object");
        bytes.push(7);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        for value in [0x21_u16, 2, 4, 0, 0, 0, 0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn reads_java_8_class_metadata() {
        let class = read_class(&minimal_class());
        assert!(class.is_ok());
        let class = class.ok();
        assert_eq!(
            class.as_ref().map(|value| value.binary_name.as_str()),
            Some("pkg.Test")
        );
        assert_eq!(class.map(|value| value.major_version), Some(52));
    }

    #[test]
    fn indexes_class_entries_inside_jar() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        assert!(
            writer
                .start_file("pkg/Test.class", zip::write::SimpleFileOptions::default())
                .is_ok()
        );
        assert!(writer.write_all(&minimal_class()).is_ok());
        let archive = match writer.finish() {
            Ok(cursor) => cursor.into_inner(),
            Err(error) => panic!("failed to create test jar: {error}"),
        };
        let classes = index_jar_reader(Cursor::new(archive), 10);
        assert_eq!(classes.ok().map(|items| items.len()), Some(1));
    }
}
