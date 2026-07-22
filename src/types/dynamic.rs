/// Statically recognized kind of an `invokedynamic` call site.
///
/// Classification reads bootstrap metadata only. It does not execute a
/// bootstrap method or initialize a Java class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicCallKind {
    /// A lambda or method-reference call site built by `LambdaMetafactory`.
    LambdaMetafactory,
    /// A string concatenation call site built by `StringConcatFactory`.
    StringConcatFactory,
    /// A call site with another bootstrap method.
    OtherBootstrap,
}
