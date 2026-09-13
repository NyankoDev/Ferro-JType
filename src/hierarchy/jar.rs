use std::{
    fs::File,
    io::{Cursor, Read, Seek},
    path::Path,
};

use super::ClassHierarchy;
use crate::Error;

impl ClassHierarchy {
    /// Adds class headers from a caller-supplied reference JAR.
    ///
    /// Accepts `rt.jar`, dependency JARs, and archives of classes exported from
    /// `jrt:/`. Entry paths may include module prefixes; the internal class name
    /// is read from each header. Resources, module descriptors, and versioned
    /// `META-INF` entries are ignored, so multi-release JARs use their base classes.
    /// Later inserts replace earlier definitions with the same internal name.
    ///
    /// Returns the number of distinct class headers read. On error the hierarchy
    /// is unchanged. Method bodies are not analyzed and Java code is never executed.
    pub fn insert_jar(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        self.insert_jar_reader(Cursor::new(bytes))
    }

    /// Reads reference class headers from an explicitly selected JAR file.
    ///
    /// Has the same indexing and error behavior as [`Self::insert_jar`].
    pub fn insert_jar_file(&mut self, path: impl AsRef<Path>) -> Result<usize, Error> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|error| reference_error(&path.display().to_string(), error))?;
        self.insert_jar_reader(file)
    }

    fn insert_jar_reader(&mut self, reader: impl Read + Seek) -> Result<usize, Error> {
        let mut archive =
            zip::ZipArchive::new(reader).map_err(|error| reference_error("<archive>", error))?;
        let mut headers = Self::new();
        let mut bytes = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| reference_error(&format!("entry #{index}"), error))?;
            let name = entry.name().to_owned();
            if !name.ends_with(".class")
                || name.starts_with("META-INF/")
                || name.rsplit('/').next() == Some("module-info.class")
            {
                continue;
            }
            bytes.clear();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| reference_error(&name, error))?;
            headers
                .insert_class(&bytes)
                .map_err(|error| reference_error(&name, error))?;
        }
        let count = headers.len();
        self.parents.extend(headers.parents);
        Ok(count)
    }
}

fn reference_error(entry: &str, source: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::ReferenceLibrary {
        entry: entry.to_owned(),
        source: Box::new(source),
    }
}
