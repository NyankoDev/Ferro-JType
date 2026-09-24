use super::{ClassName, PrimitiveType};

/// A generic JVM signature preserved from a `Signature` attribute.
///
/// Generic signatures complement erased descriptors. They are metadata rather
/// than verifier input, so inference remains valid when they are absent or
/// malformed. The value uses JVM generic-signature syntax verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericSignature(String);

impl GenericSignature {
    /// Creates a generic signature from JVM signature text.
    #[must_use]
    pub fn parse(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Returns the generic signature in JVM signature syntax.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the signature and returns its JVM signature text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// Parses a field type signature into a typed generic representation.
    ///
    /// Returns `None` for malformed signatures or signatures that contain a
    /// method/class declaration rather than one field type.
    #[must_use]
    pub fn parse_field_type(&self) -> Option<GenericType> {
        let mut parser = Parser {
            input: self.0.as_bytes(),
            cursor: 0,
        };
        let ty = parser.parse_type()?;
        (parser.cursor == parser.input.len()).then_some(ty)
    }

    /// Parses a class generic signature.
    #[must_use]
    pub fn parse_class(&self) -> Option<GenericClassSignature> {
        let mut parser = Parser {
            input: self.0.as_bytes(),
            cursor: 0,
        };
        let parameters = parser.formals()?;
        let superclass = parser.class_type()?;
        let mut interfaces = Vec::new();
        while parser.peek().is_some() {
            interfaces.push(parser.class_type()?);
        }
        Some(GenericClassSignature {
            parameters,
            superclass,
            interfaces,
        })
    }

    /// Parses a method generic signature.
    #[must_use]
    pub fn parse_method(&self) -> Option<GenericMethodSignature> {
        let mut parser = Parser {
            input: self.0.as_bytes(),
            cursor: 0,
        };
        let parameters = parser.formals()?;
        parser.consume(b'(').then_some(())?;
        let mut arguments = Vec::new();
        while !parser.consume(b')') {
            arguments.push(parser.parse_type()?);
        }
        let returns = if parser.consume(b'V') {
            None
        } else {
            Some(parser.parse_type()?)
        };
        let mut throws = Vec::new();
        while parser.consume(b'^') {
            throws.push(parser.parse_type()?);
        }
        (parser.cursor == parser.input.len()).then_some(GenericMethodSignature {
            parameters,
            arguments,
            returns,
            throws,
        })
    }
}

/// Formal parameters and bounds from a class or method signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericParameter {
    /// Type-variable identifier.
    pub name: String,
    /// Explicit upper bounds.
    pub bounds: Vec<GenericType>,
}

/// Parsed class generic signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericClassSignature {
    /// Formal type parameters.
    pub parameters: Vec<GenericParameter>,
    /// Generic superclass.
    pub superclass: GenericClassType,
    /// Generic superinterfaces.
    pub interfaces: Vec<GenericClassType>,
}

/// Parsed method generic signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericMethodSignature {
    /// Formal type parameters.
    pub parameters: Vec<GenericParameter>,
    /// Generic argument types.
    pub arguments: Vec<GenericType>,
    /// Generic result type, or `None` for void.
    pub returns: Option<GenericType>,
    /// Declared thrown types.
    pub throws: Vec<GenericType>,
}

/// A parsed JVM field type that preserves generic arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GenericType {
    /// A primitive JVM type.
    Primitive(PrimitiveType),
    /// A parameterized or ordinary class type.
    Class(GenericClassType),
    /// An array type.
    Array(Box<GenericType>),
    /// A type-variable reference such as `TE;`.
    Variable(String),
}

/// A parsed generic class type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericClassType {
    /// JVM internal class name of the first class segment.
    pub name: ClassName,
    /// Type arguments on the first class segment.
    pub arguments: Vec<GenericArgument>,
    /// Nested class segments following the first segment.
    pub nested: Vec<GenericClassSegment>,
}

/// A nested class segment in a generic class type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericClassSegment {
    /// Source-level nested class name.
    pub name: String,
    /// Type arguments on this nested segment.
    pub arguments: Vec<GenericArgument>,
}

/// An argument in a generic type argument list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GenericArgument {
    /// A concrete generic type argument.
    Type(GenericType),
    /// An unbounded, extends-bounded, or super-bounded wildcard.
    Wildcard(GenericWildcard),
}

/// A Java wildcard bound.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GenericWildcard {
    /// An unbounded wildcard (`?`).
    Unbounded,
    /// An extends wildcard (`? extends T`).
    Extends(GenericType),
    /// A super wildcard (`? super T`).
    Super(GenericType),
}

struct Parser<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn class_type(&mut self) -> Option<GenericClassType> {
        if !self.consume(b'L') {
            return None;
        }
        self.parse_class()
    }

    fn formals(&mut self) -> Option<Vec<GenericParameter>> {
        let mut parameters = Vec::new();
        if !self.consume(b'<') {
            return Some(parameters);
        }
        while !self.consume(b'>') {
            let name = self.until(b':')?;
            let mut bounds = Vec::new();
            if self.peek() != Some(b':') {
                bounds.push(self.parse_type()?);
            }
            while self.consume(b':') {
                bounds.push(self.parse_type()?);
            }
            parameters.push(GenericParameter { name, bounds });
        }
        Some(parameters)
    }

    fn parse_type(&mut self) -> Option<GenericType> {
        let tag = self.next()?;
        Some(match tag {
            b'Z' => GenericType::Primitive(PrimitiveType::Boolean),
            b'B' => GenericType::Primitive(PrimitiveType::Byte),
            b'C' => GenericType::Primitive(PrimitiveType::Char),
            b'S' => GenericType::Primitive(PrimitiveType::Short),
            b'I' => GenericType::Primitive(PrimitiveType::Int),
            b'F' => GenericType::Primitive(PrimitiveType::Float),
            b'J' => GenericType::Primitive(PrimitiveType::Long),
            b'D' => GenericType::Primitive(PrimitiveType::Double),
            b'T' => {
                let name = self.until(b';')?;
                self.consume(b';').then_some(GenericType::Variable(name))?
            }
            b'[' => GenericType::Array(Box::new(self.parse_type()?)),
            b'L' => GenericType::Class(self.parse_class()?),
            _ => return None,
        })
    }

    fn parse_class(&mut self) -> Option<GenericClassType> {
        let name = self.until_any(&[b'<', b'.', b';'])?;
        let name = ClassName::parse(name).ok()?;
        let arguments = self.parse_arguments()?;
        let mut nested = Vec::new();
        while self.consume(b'.') {
            let name = self.until_any(&[b'<', b'.', b';'])?;
            let arguments = self.parse_arguments()?;
            nested.push(GenericClassSegment { name, arguments });
        }
        self.consume(b';').then_some(GenericClassType {
            name,
            arguments,
            nested,
        })
    }

    fn parse_arguments(&mut self) -> Option<Vec<GenericArgument>> {
        if !self.consume(b'<') {
            return Some(Vec::new());
        }
        let mut arguments = Vec::new();
        while !self.consume(b'>') {
            let argument = match self.next()? {
                b'*' => GenericArgument::Wildcard(GenericWildcard::Unbounded),
                b'+' => GenericArgument::Wildcard(GenericWildcard::Extends(self.parse_type()?)),
                b'-' => GenericArgument::Wildcard(GenericWildcard::Super(self.parse_type()?)),
                _ => {
                    self.cursor -= 1;
                    GenericArgument::Type(self.parse_type()?)
                }
            };
            arguments.push(argument);
        }
        Some(arguments)
    }

    fn until(&mut self, terminator: u8) -> Option<String> {
        self.until_any(&[terminator])
    }

    fn until_any(&mut self, terminators: &[u8]) -> Option<String> {
        let start = self.cursor;
        while let Some(byte) = self.input.get(self.cursor) {
            if terminators.contains(byte) {
                break;
            }
            self.cursor += 1;
        }
        (start != self.cursor)
            .then(|| String::from_utf8(self.input[start..self.cursor].to_vec()).ok())?
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.input.get(self.cursor) == Some(&expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<u8> {
        let byte = *self.input.get(self.cursor)?;
        self.cursor += 1;
        Some(byte)
    }
}
