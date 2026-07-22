use crate::{ClassName, DescriptorError};

/// A JVM primitive type from a field or method descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    /// The `Z` descriptor type.
    Boolean,
    /// The `B` descriptor type.
    Byte,
    /// The `C` descriptor type.
    Char,
    /// The `S` descriptor type.
    Short,
    /// The `I` descriptor type.
    Int,
    /// The `F` descriptor type.
    Float,
    /// The `J` descriptor type.
    Long,
    /// The `D` descriptor type.
    Double,
}

impl PrimitiveType {
    /// Returns the number of JVM local-variable or operand-stack slots used.
    ///
    /// `long` and `double` use two slots; every other primitive uses one.
    #[must_use]
    pub const fn slot_width(self) -> u8 {
        match self {
            Self::Long | Self::Double => 2,
            Self::Boolean | Self::Byte | Self::Char | Self::Short | Self::Int | Self::Float => 1,
        }
    }
}

/// A non-void JVM field type descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDescriptor {
    /// A primitive descriptor type.
    Primitive(PrimitiveType),
    /// A reference to a class identified by its JVM internal name.
    Reference(ClassName),
    /// An array descriptor with its dimension count and element descriptor.
    Array {
        /// Number of leading `[` characters in the descriptor.
        dimensions: u8,
        /// Type stored at the array's innermost dimension.
        element: Box<TypeDescriptor>,
    },
}

impl TypeDescriptor {
    /// Parses one complete non-void JVM field descriptor.
    ///
    /// For example, `I` describes an integer and `[Ljava/lang/String;`
    /// describes an array of strings.
    pub fn parse(input: &str) -> Result<Self, DescriptorError> {
        let mut parser = DescriptorParser::new(input);
        let descriptor = parser.parse_field_type()?;

        if !parser.is_finished() {
            return Err(DescriptorError::TrailingInput {
                offset: parser.offset(),
            });
        }

        Ok(descriptor)
    }

    /// Returns the number of JVM local-variable or operand-stack slots used.
    #[must_use]
    pub const fn slot_width(&self) -> u8 {
        match self {
            Self::Primitive(primitive) => primitive.slot_width(),
            Self::Reference(_) | Self::Array { .. } => 1,
        }
    }
}

/// A JVM method return descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReturnType {
    /// The `V` method return descriptor.
    Void,
    /// A method that returns a non-void value.
    Type(TypeDescriptor),
}

/// A parsed JVM method descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodDescriptor {
    parameters: Vec<TypeDescriptor>,
    return_type: ReturnType,
}

impl MethodDescriptor {
    /// Parses one complete JVM method descriptor.
    ///
    /// For example, `(I)Ljava/lang/String;` describes a method that accepts an
    /// integer and returns a string.
    pub fn parse(input: &str) -> Result<Self, DescriptorError> {
        let mut parser = DescriptorParser::new(input);
        parser.expect('(')?;

        let mut parameters = Vec::new();
        while !parser.consume(')')? {
            parameters.push(parser.parse_field_type()?);
        }

        let return_type = if parser.consume('V')? {
            ReturnType::Void
        } else {
            ReturnType::Type(parser.parse_field_type()?)
        };

        if !parser.is_finished() {
            return Err(DescriptorError::TrailingInput {
                offset: parser.offset(),
            });
        }

        Ok(Self {
            parameters,
            return_type,
        })
    }

    /// Returns parameter descriptors in declaration order.
    #[must_use]
    pub fn parameters(&self) -> &[TypeDescriptor] {
        &self.parameters
    }

    /// Returns the method's return descriptor.
    #[must_use]
    pub const fn return_type(&self) -> &ReturnType {
        &self.return_type
    }

    /// Returns the combined JVM slot width of all parameters.
    ///
    /// This does not include an instance method's implicit `this` parameter.
    #[must_use]
    pub fn parameter_slot_count(&self) -> u16 {
        self.parameters
            .iter()
            .map(|parameter| u16::from(parameter.slot_width()))
            .sum()
    }
}

struct DescriptorParser<'input> {
    input: &'input [u8],
    cursor: usize,
}

impl<'input> DescriptorParser<'input> {
    const fn new(input: &'input str) -> Self {
        Self {
            input: input.as_bytes(),
            cursor: 0,
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), DescriptorError> {
        if self.consume(expected)? {
            Ok(())
        } else {
            Err(DescriptorError::Expected {
                expected,
                offset: self.cursor,
            })
        }
    }

    fn consume(&mut self, expected: char) -> Result<bool, DescriptorError> {
        let Some(actual) = self.peek()? else {
            return Ok(false);
        };

        if actual == expected as u8 {
            self.cursor += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn parse_field_type(&mut self) -> Result<TypeDescriptor, DescriptorError> {
        let offset = self.cursor;
        let tag = self
            .next()?
            .ok_or(DescriptorError::UnexpectedEnd { offset })?;

        let primitive = match tag {
            b'Z' => Some(PrimitiveType::Boolean),
            b'B' => Some(PrimitiveType::Byte),
            b'C' => Some(PrimitiveType::Char),
            b'S' => Some(PrimitiveType::Short),
            b'I' => Some(PrimitiveType::Int),
            b'F' => Some(PrimitiveType::Float),
            b'J' => Some(PrimitiveType::Long),
            b'D' => Some(PrimitiveType::Double),
            _ => None,
        };

        if let Some(primitive) = primitive {
            return Ok(TypeDescriptor::Primitive(primitive));
        }

        match tag {
            b'L' => self.parse_reference_type(),
            b'[' => self.parse_array_type(),
            tag => Err(DescriptorError::InvalidTag {
                tag: char::from(tag),
                offset,
            }),
        }
    }

    fn parse_reference_type(&mut self) -> Result<TypeDescriptor, DescriptorError> {
        let start = self.cursor;
        while let Some(next) = self.peek()? {
            if next == b';' {
                break;
            }
            self.cursor += 1;
        }

        if self.cursor == self.input.len() {
            return Err(DescriptorError::UnexpectedEnd {
                offset: self.cursor,
            });
        }

        let name = std::str::from_utf8(&self.input[start..self.cursor]).map_err(|_| {
            DescriptorError::InvalidTag {
                tag: '\u{fffd}',
                offset: start,
            }
        })?;
        self.cursor += 1;

        Ok(TypeDescriptor::Reference(ClassName::parse(name)?))
    }

    fn parse_array_type(&mut self) -> Result<TypeDescriptor, DescriptorError> {
        let mut dimensions = 1_u8;
        while self.consume('[')? {
            dimensions = dimensions
                .checked_add(1)
                .ok_or(DescriptorError::TooManyArrayDimensions)?;
        }

        let element = self.parse_field_type()?;
        Ok(TypeDescriptor::Array {
            dimensions,
            element: Box::new(element),
        })
    }

    fn peek(&self) -> Result<Option<u8>, DescriptorError> {
        Ok(self.input.get(self.cursor).copied())
    }

    fn next(&mut self) -> Result<Option<u8>, DescriptorError> {
        let next = self.peek()?;
        if next.is_some() {
            self.cursor += 1;
        }
        Ok(next)
    }

    const fn offset(&self) -> usize {
        self.cursor
    }

    const fn is_finished(&self) -> bool {
        self.cursor == self.input.len()
    }
}
