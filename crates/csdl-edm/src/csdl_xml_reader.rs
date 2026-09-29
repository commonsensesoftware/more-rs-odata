use std::collections::VecDeque;
use std::io::{self, BufRead, Read};

use quick_xml::events::{BytesStart, Event};

use crate::csdl::Annotation;
use crate::error::{Error, Result};
use crate::expr::{BinaryOperator, CsdlAnnotationExpression, PropertyValue};

/// Token stream produced by [`CsdlReader`]. The reader normalizes CSDL's
/// inline-attribute vs nested-element duality (see CSDL 14.2 and following)
/// so consumers see a single shape.
/// it also normalizes self closing xml elements into a Start- and End
/// element so that consumers can process a uniform "shape"
///
/// Names and attribute values are owned `String`s. The reader has to copy
/// them out of `quick-xml`'s scratch buffer (which is reused across reads)
/// anyway, and downstream consumers (the syntactic-model builder, callers of
/// the public reader API) all want owned values.
#[derive(Debug)]
pub enum SyntaxUnit {
    /// Opening tag of a XML CSDL *element* (Schema, EntityType, Annotation, ...).
    /// When the element is `<Annotation>`, any inline-attribute annotation
    /// expression (e.g. `String="x"`) is **removed** from `attributes` and
    /// emitted instead as a following [`SyntaxUnit::AnnotationExpression`].
    StartElement {
        name: String,
        attributes: Vec<(String, String)>,
    },
    /// Closing tag of a XML CSDL element.
    EndElement { name: String },
    /// A single CSDL annotation *expression* — distinct from a CSDL element.
    /// Emitted only between Start/End of an element that carries annotation
    /// expressions, which per CSDL 4.01 is `Annotation` (14.2) or
    /// `PropertyValue` (14.5.14). Multiple consecutive
    /// `AnnotationExpression` tokens between one Start/End pair indicate
    /// the source had both inline and nested forms, or multiple nested
    /// expressions; the semantic layer decides validity.
    AnnotationExpression(CsdlAnnotationExpression),
}

/// 1-based line and column of the most recently consumed position in the
/// CSDL source. Column counts bytes within its line; for ASCII CSDL it
/// matches the character column. Multibyte UTF-8 in attribute values or text
/// content would shift it — fine for error pointers, not for cursor math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: u32,
    pub column: u32,
}

/// `BufRead` wrapper that maintains a running (line, column) cursor by
/// scanning bytes as they're consumed by the inner reader. `\n` advances the
/// line and resets the column; every other byte advances the column.
struct LocationBufRead<R> {
    inner: R,
    line: u32,
    column: u32,
}

impl<R> LocationBufRead<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            line: 1,
            column: 1,
        }
    }

    fn location(&self) -> Location {
        Location {
            line: self.line,
            column: self.column,
        }
    }
}

impl<R: BufRead> BufRead for LocationBufRead<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        if amt > 0 {
            if let Ok(buf) = self.inner.fill_buf() {
                let n = amt.min(buf.len());
                for &b in &buf[..n] {
                    if b == b'\n' {
                        self.line += 1;
                        self.column = 1;
                    } else {
                        self.column += 1;
                    }
                }
            }
        }
        self.inner.consume(amt);
    }
}

impl<R: BufRead> Read for LocationBufRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let avail = self.fill_buf()?;
        let n = avail.len().min(buf.len());
        buf[..n].copy_from_slice(&avail[..n]);
        self.consume(n);
        Ok(n)
    }
}

pub struct CsdlReader<R: BufRead> {
    inner: quick_xml::Reader<LocationBufRead<R>>,
    buf: Vec<u8>,
    /// Tokens queued during a complex element (Annotation / PropertyValue).
    /// When non-empty, [`Self::next_token`] drains this before reading more
    /// XML events.
    deferred: VecDeque<SyntaxUnit>,
}

impl<R: BufRead> CsdlReader<R> {
    pub fn from_reader(r: R) -> Self {
        let mut inner = quick_xml::Reader::from_reader(LocationBufRead::new(r));
        inner.config_mut().trim_text(true);
        Self {
            inner,
            buf: Vec::new(),
            deferred: VecDeque::new(),
        }
    }

    /// Current source position — the line and column the underlying reader
    /// has consumed up to. After a successful event read this points just
    /// past the end of that event, so it is appropriate for error messages
    /// emitted while or after processing the most recent event.
    pub fn current_location(&self) -> Location {
        self.inner.get_ref().location()
    }

    fn csdl_err(&self, msg: impl Into<String>) -> Error {
        let loc = self.current_location();
        Error::Csdl(format!(
            "at line {}, column {}: {}",
            loc.line,
            loc.column,
            msg.into()
        ))
    }

    pub fn next_unit(&mut self) -> Result<Option<SyntaxUnit>> {
        if let Some(d) = self.deferred.pop_front() {
            return Ok(Some(d));
        }

        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::Eof => return Ok(None),
                Event::Start(e) => {
                    let (name, mut attrs) = parse_start(&e)?;

                    if name == "Annotation" {
                        // Inline-attribute expression (if any) + every nested
                        // expression in the body — all emitted as a sequence.
                        // An omitted expression uses the term's default. Preserve
                        // absence until the term is resolved.
                        let mut exprs: Vec<CsdlAnnotationExpression> = Vec::new();
                        if let Some(idx) = find_inline_constant_idx(&attrs) {
                            let (k, v) = attrs.swap_remove(idx);
                            exprs.push(constant_attr_to_expr(&k, v));
                        }
                        exprs.extend(self.read_expression_list("Annotation")?);
                        for ex in exprs {
                            self.deferred
                                .push_back(SyntaxUnit::AnnotationExpression(ex));
                        }
                        self.deferred.push_back(SyntaxUnit::EndElement {
                            name: "Annotation".to_string(),
                        });
                        return Ok(Some(SyntaxUnit::StartElement {
                            name,
                            attributes: attrs,
                        }));
                    }

                    return Ok(Some(SyntaxUnit::StartElement {
                        name,
                        attributes: attrs,
                    }));
                }
                Event::End(e) => {
                    let name = local_name_string(e.name().as_ref())?;
                    return Ok(Some(SyntaxUnit::EndElement { name }));
                }
                Event::Empty(e) => {
                    let (name, mut attrs) = parse_start(&e)?;

                    if name == "Annotation" {
                        if let Some(idx) = find_inline_constant_idx(&attrs) {
                            let (k, v) = attrs.swap_remove(idx);
                            self.deferred.push_back(SyntaxUnit::AnnotationExpression(
                                constant_attr_to_expr(&k, v),
                            ));
                        }
                        self.deferred.push_back(SyntaxUnit::EndElement {
                            name: "Annotation".to_string(),
                        });
                        return Ok(Some(SyntaxUnit::StartElement {
                            name,
                            attributes: attrs,
                        }));
                    }

                    self.deferred
                        .push_back(SyntaxUnit::EndElement { name: name.clone() });
                    return Ok(Some(SyntaxUnit::StartElement {
                        name,
                        attributes: attrs,
                    }));
                }
                _ => continue,
            }
        }
    }

    fn build_expr_from_start(
        &mut self,
        name: &str,
        attrs: Vec<(String, String)>,
    ) -> Result<CsdlAnnotationExpression> {
        match name {
            "Binary" => Ok(CsdlAnnotationExpression::Binary(
                self.read_text_body(name)?.into_bytes(),
            )),
            "Bool" => {
                let t = self.read_text_body(name)?;
                Ok(CsdlAnnotationExpression::Bool(
                    t.eq_ignore_ascii_case("true"),
                ))
            }
            "Date" => Ok(CsdlAnnotationExpression::Date(self.read_text_body(name)?)),
            "DateTimeOffset" => Ok(CsdlAnnotationExpression::DateTimeOffset(
                self.read_text_body(name)?,
            )),
            "Decimal" => Ok(CsdlAnnotationExpression::Decimal(
                self.read_text_body(name)?,
            )),
            "Duration" => Ok(CsdlAnnotationExpression::Duration(
                self.read_text_body(name)?,
            )),
            "EnumMember" => Ok(CsdlAnnotationExpression::EnumMember(
                self.read_text_body(name)?,
            )),
            "Float" => Ok(CsdlAnnotationExpression::Float(
                self.read_text_body(name)?.parse().unwrap_or(0.0),
            )),
            "Guid" => Ok(CsdlAnnotationExpression::Guid(self.read_text_body(name)?)),
            "Int" => Ok(CsdlAnnotationExpression::Int(
                self.read_text_body(name)?.parse().unwrap_or(0),
            )),
            "String" => Ok(CsdlAnnotationExpression::String(self.read_text_body(name)?)),
            "TimeOfDay" => Ok(CsdlAnnotationExpression::TimeOfDay(
                self.read_text_body(name)?,
            )),

            "Path" => Ok(CsdlAnnotationExpression::Path(self.read_text_body(name)?)),
            "PropertyPath" => Ok(CsdlAnnotationExpression::PropertyPath(
                self.read_text_body(name)?,
            )),
            "NavigationPropertyPath" => Ok(CsdlAnnotationExpression::NavigationPropertyPath(
                self.read_text_body(name)?,
            )),
            "AnnotationPath" => Ok(CsdlAnnotationExpression::AnnotationPath(
                self.read_text_body(name)?,
            )),
            "LabeledElementReference" => Ok(CsdlAnnotationExpression::LabeledElementReference(
                self.read_text_body(name)?,
            )),

            "Not" => {
                let c = self.read_expression_list(name)?;
                let [inner] = exact::<1>(name, c)?;
                Ok(CsdlAnnotationExpression::Not(Box::new(inner)))
            }
            "And" | "Or" | "Eq" | "Ne" | "Gt" | "Ge" | "Lt" | "Le" => {
                let c = self.read_expression_list(name)?;
                let [lhs, rhs] = exact::<2>(name, c)?;
                Ok(make_binop(name, lhs, rhs))
            }

            "If" => {
                let mut c = self.read_expression_list(name)?;
                if c.len() != 2 && c.len() != 3 {
                    return Err(
                        self.csdl_err(format!("<If> expects 2 or 3 children, got {}", c.len()))
                    );
                }
                let else_ = if c.len() == 3 {
                    Some(Box::new(c.swap_remove(2)))
                } else {
                    None
                };
                let then_ = Box::new(c.swap_remove(1));
                let test = Box::new(c.swap_remove(0));
                Ok(CsdlAnnotationExpression::If { test, then_, else_ })
            }

            "Apply" => {
                let function = attr_value(&attrs, "Function").unwrap_or_default();
                let args = self.read_expression_list(name)?;
                Ok(CsdlAnnotationExpression::Apply { function, args })
            }

            "Cast" => {
                let type_ = attr_value(&attrs, "Type").unwrap_or_default();
                let c = self.read_expression_list(name)?;
                let [expr] = exact::<1>(name, c)?;
                Ok(CsdlAnnotationExpression::Cast {
                    type_,
                    expr: Box::new(expr),
                })
            }

            "IsOf" => {
                let type_ = attr_value(&attrs, "Type").unwrap_or_default();
                let c = self.read_expression_list(name)?;
                let [expr] = exact::<1>(name, c)?;
                Ok(CsdlAnnotationExpression::IsOf {
                    type_,
                    expr: Box::new(expr),
                })
            }

            "LabeledElement" => {
                let label = attr_value(&attrs, "Name").unwrap_or_default();
                let c = self.read_expression_list(name)?;
                let [expr] = exact::<1>(name, c)?;
                Ok(CsdlAnnotationExpression::LabeledElement {
                    name: label,
                    expr: Box::new(expr),
                })
            }

            "UrlRef" => {
                let c = self.read_expression_list(name)?;
                let [expr] = exact::<1>(name, c)?;
                Ok(CsdlAnnotationExpression::UrlRef(Box::new(expr)))
            }

            "Collection" => {
                let items = self.read_expression_list(name)?;
                Ok(CsdlAnnotationExpression::Collection(items))
            }

            "Record" => {
                let type_ = attr_value(&attrs, "Type");
                let (properties, annotations) = self.read_record_body(name)?;
                Ok(CsdlAnnotationExpression::Record {
                    type_,
                    properties,
                    annotations,
                })
            }

            "Null" => {
                self.expect_end(name)?;
                Ok(CsdlAnnotationExpression::Null)
            }

            other => Err(self.csdl_err(format!("unknown expression element <{other}>"))),
        }
    }

    fn read_text_body(&mut self, name: &str) -> Result<String> {
        let mut out = String::new();
        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::Text(t) => {
                    let decoded = t
                        .decode()
                        .map_err(|e| Error::Csdl(format!("text decode error: {e}")))?;
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .map_err(|e| Error::Csdl(format!("text unescape error: {e}")))?;
                    out.push_str(&unescaped);
                }
                Event::CData(t) => out.push_str(std::str::from_utf8(&t)?),
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == name {
                        return Ok(out);
                    }
                    return Err(
                        self.csdl_err(format!("unexpected </{n}> in text body of <{name}>"))
                    );
                }
                Event::Comment(_) => continue,
                Event::Eof => {
                    return Err(self.csdl_err(format!("unexpected EOF in text body of <{name}>")));
                }
                _ => {
                    return Err(self.csdl_err(format!("unexpected non-text event in <{name}>")));
                }
            }
        }
    }

    /// Read zero or more annotation expressions until the End tag of `parent`.
    /// Consumes the End tag.
    fn read_expression_list(&mut self, parent: &str) -> Result<Vec<CsdlAnnotationExpression>> {
        let mut out = Vec::new();
        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::Start(e) => {
                    let (n, a) = parse_start(&e)?;
                    out.push(self.build_expr_from_start(&n, a)?);
                }
                Event::Empty(e) => {
                    let (n, a) = parse_start(&e)?;
                    out.push(build_expr_from_empty(&n, a));
                }
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == parent {
                        return Ok(out);
                    }
                    return Err(self.csdl_err(format!("unexpected </{n}> in <{parent}>")));
                }
                Event::Text(_) | Event::CData(_) | Event::Comment(_) => continue,
                Event::Eof => return Err(self.csdl_err(format!("unexpected EOF in <{parent}>"))),
                _ => continue,
            }
        }
    }

    /// Read the body of `<Record>` until `</Record>`: any mix of
    /// `<PropertyValue>` and `<Annotation>` children. Per CSDL 14.5.14
    /// PropertyValue carries its own annotation expression(s) and optional
    /// nested `<Annotation>` elements; Record itself can also carry
    /// `<Annotation>` children.
    fn read_record_body(&mut self, parent: &str) -> Result<(Vec<PropertyValue>, Vec<Annotation>)> {
        let mut properties = Vec::new();
        let mut annotations = Vec::new();
        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::Start(e) => {
                    let (n, mut a) = parse_start(&e)?;
                    match n.as_str() {
                        "PropertyValue" => {
                            properties.push(self.read_property_value_start(&mut a)?);
                        }
                        "Annotation" => {
                            annotations.push(self.read_annotation_start(&mut a)?);
                        }
                        _ => {
                            return Err(self.csdl_err(format!(
                                "unexpected <{n}> in <{parent}> (expected PropertyValue or Annotation)"
                            )));
                        }
                    }
                }
                Event::Empty(e) => {
                    let (n, mut a) = parse_start(&e)?;
                    match n.as_str() {
                        "PropertyValue" => {
                            let property = attr_value(&a, "Property").unwrap_or_default();
                            let value = find_inline_constant_idx(&a).map(|idx| {
                                let (k, v) = a.swap_remove(idx);
                                constant_attr_to_expr(&k, v)
                            });
                            properties.push(PropertyValue {
                                property,
                                value,
                                annotations: Vec::new(),
                            });
                        }
                        "Annotation" => {
                            annotations.push(self.read_annotation_empty(&mut a));
                        }
                        _ => {
                            return Err(self.csdl_err(format!(
                                "unexpected <{n}/> in <{parent}> (expected PropertyValue or Annotation)"
                            )));
                        }
                    }
                }
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == parent {
                        return Ok((properties, annotations));
                    }
                    return Err(self.csdl_err(format!("unexpected </{n}> in <{parent}>")));
                }
                Event::Text(_) | Event::CData(_) | Event::Comment(_) => continue,
                Event::Eof => return Err(self.csdl_err(format!("unexpected EOF in <{parent}>"))),
                _ => continue,
            }
        }
    }

    /// Consume the rest of a `<PropertyValue ...>` element (after its Start
    /// event is already parsed). Body can mix value expressions and nested
    /// `<Annotation>` children. The first value expression wins (inline
    /// attribute > nested element); excess are dropped.
    fn read_property_value_start(
        &mut self,
        attrs: &mut Vec<(String, String)>,
    ) -> Result<PropertyValue> {
        let property = attr_value(attrs, "Property").unwrap_or_default();
        let inline = find_inline_constant_idx(attrs).map(|idx| {
            let (k, v) = attrs.swap_remove(idx);
            constant_attr_to_expr(&k, v)
        });
        let mut body_values: Vec<CsdlAnnotationExpression> = Vec::new();
        let mut annotations: Vec<Annotation> = Vec::new();
        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::Start(e) => {
                    let (n, mut a) = parse_start(&e)?;
                    if n == "Annotation" {
                        annotations.push(self.read_annotation_start(&mut a)?);
                    } else {
                        body_values.push(self.build_expr_from_start(&n, a)?);
                    }
                }
                Event::Empty(e) => {
                    let (n, mut a) = parse_start(&e)?;
                    if n == "Annotation" {
                        annotations.push(self.read_annotation_empty(&mut a));
                    } else {
                        body_values.push(build_expr_from_empty(&n, a));
                    }
                }
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == "PropertyValue" {
                        let value = pick_first_value(inline, body_values);
                        return Ok(PropertyValue {
                            property,
                            value,
                            annotations,
                        });
                    }
                    return Err(self.csdl_err(format!("unexpected </{n}> in <PropertyValue>")));
                }
                Event::Text(_) | Event::CData(_) | Event::Comment(_) => continue,
                Event::Eof => return Err(self.csdl_err("unexpected EOF in <PropertyValue>")),
                _ => continue,
            }
        }
    }

    /// Build a model `Annotation` from a `<Annotation Term="..." [Target=..]
    /// [Qualifier=..] [InlineConstantAttr=..]> ... </Annotation>` element.
    /// Body may carry one value expression (taken as `expression`).
    fn read_annotation_start(&mut self, attrs: &mut Vec<(String, String)>) -> Result<Annotation> {
        let term = attr_value(attrs, "Term").unwrap_or_default();
        let qualifier = attr_value(attrs, "Qualifier");
        let target = attr_value(attrs, "Target");
        // Inline-constant attribute (e.g. String="x"), if present.
        let inline = find_inline_constant_idx(attrs).map(|idx| {
            let (k, v) = attrs.swap_remove(idx);
            constant_attr_to_expr(&k, v)
        });
        let body = self.read_expression_list("Annotation")?;
        let expression = match inline {
            Some(e) => Some(e),
            None if body.is_empty() => None,
            None => Some(body.into_iter().next().unwrap()),
        };
        Ok(Annotation {
            term,
            qualifier,
            target,
            expression,
        })
    }

    /// Empty-element form (`<Annotation Term=".."/>`). Marker form
    /// (no inline constant attr) preserves the absence of an expression.
    fn read_annotation_empty(&mut self, attrs: &mut Vec<(String, String)>) -> Annotation {
        let term = attr_value(attrs, "Term").unwrap_or_default();
        let qualifier = attr_value(attrs, "Qualifier");
        let target = attr_value(attrs, "Target");
        let expression = match find_inline_constant_idx(attrs) {
            Some(idx) => {
                let (k, v) = attrs.swap_remove(idx);
                Some(constant_attr_to_expr(&k, v))
            }
            None => None,
        };
        Annotation {
            term,
            qualifier,
            target,
            expression,
        }
    }

    fn expect_end(&mut self, name: &str) -> Result<()> {
        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == name {
                        return Ok(());
                    }
                    return Err(self.csdl_err(format!("expected </{name}>, got </{n}>")));
                }
                Event::Text(_) | Event::CData(_) | Event::Comment(_) => continue,
                Event::Eof => {
                    return Err(self.csdl_err(format!("EOF while expecting </{name}>")));
                }
                _ => {
                    return Err(
                        self.csdl_err(format!("unexpected event while expecting </{name}>"))
                    );
                }
            }
        }
    }
}

/// Per CSDL 14.5.14 a PropertyValue carries one annotation expression.
/// If both inline attribute and nested element forms are present, the inline
/// form wins (it appeared first lexically on the start tag); any nested
/// expressions beyond the first body expression are silently dropped.
/// Surfacing this as a warning is tracked in `TODO/structured-warnings.md`.
fn pick_first_value(
    inline: Option<CsdlAnnotationExpression>,
    mut body: Vec<CsdlAnnotationExpression>,
) -> Option<CsdlAnnotationExpression> {
    match inline {
        Some(v) => Some(v),
        None if body.is_empty() => None,
        None => Some(body.swap_remove(0)),
    }
}

fn exact<const N: usize>(
    name: &str,
    v: Vec<CsdlAnnotationExpression>,
) -> Result<[CsdlAnnotationExpression; N]> {
    let got = v.len();
    v.try_into()
        .map_err(|_| Error::Csdl(format!("<{name}> expects {N} children, got {got}")))
}

fn build_expr_from_empty(name: &str, attrs: Vec<(String, String)>) -> CsdlAnnotationExpression {
    match name {
        "Null" => CsdlAnnotationExpression::Null,
        "Binary" => CsdlAnnotationExpression::Binary(Vec::new()),
        "Bool" => CsdlAnnotationExpression::Bool(false),
        "Date" => CsdlAnnotationExpression::Date(String::new()),
        "DateTimeOffset" => CsdlAnnotationExpression::DateTimeOffset(String::new()),
        "Decimal" => CsdlAnnotationExpression::Decimal(String::new()),
        "Duration" => CsdlAnnotationExpression::Duration(String::new()),
        "EnumMember" => CsdlAnnotationExpression::EnumMember(String::new()),
        "Float" => CsdlAnnotationExpression::Float(0.0),
        "Guid" => CsdlAnnotationExpression::Guid(String::new()),
        "Int" => CsdlAnnotationExpression::Int(0),
        "String" => CsdlAnnotationExpression::String(String::new()),
        "TimeOfDay" => CsdlAnnotationExpression::TimeOfDay(String::new()),
        "Path" => CsdlAnnotationExpression::Path(String::new()),
        "PropertyPath" => CsdlAnnotationExpression::PropertyPath(String::new()),
        "NavigationPropertyPath" => CsdlAnnotationExpression::NavigationPropertyPath(String::new()),
        "AnnotationPath" => CsdlAnnotationExpression::AnnotationPath(String::new()),
        "LabeledElementReference" => {
            CsdlAnnotationExpression::LabeledElementReference(String::new())
        }
        "Collection" => CsdlAnnotationExpression::Collection(Vec::new()),
        "Record" => CsdlAnnotationExpression::Record {
            type_: attr_value(&attrs, "Type"),
            properties: Vec::new(),
            annotations: Vec::new(),
        },
        "Apply" => CsdlAnnotationExpression::Apply {
            function: attr_value(&attrs, "Function").unwrap_or_default(),
            args: Vec::new(),
        },
        _ => CsdlAnnotationExpression::Null,
    }
}

fn make_binop(
    name: &str,
    lhs: CsdlAnnotationExpression,
    rhs: CsdlAnnotationExpression,
) -> CsdlAnnotationExpression {
    let op = match name {
        "And" => BinaryOperator::And,
        "Or" => BinaryOperator::Or,
        "Eq" => BinaryOperator::Eq,
        "Ne" => BinaryOperator::Ne,
        "Gt" => BinaryOperator::Gt,
        "Ge" => BinaryOperator::Ge,
        "Lt" => BinaryOperator::Lt,
        "Le" => BinaryOperator::Le,
        _ => unreachable!(),
    };
    CsdlAnnotationExpression::BinaryExpression {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn attr_value(attrs: &[(String, String)], key: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn parse_start(e: &BytesStart<'_>) -> Result<(String, Vec<(String, String)>)> {
    let name = local_name_string(e.name().as_ref())?;
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|err| Error::Csdl(format!("attribute parse error: {err}")))?;
        let key = local_name_string(a.key.as_ref())?;
        let value = std::str::from_utf8(&a.value)?.to_string();
        attrs.push((key, value));
    }
    Ok((name, attrs))
}

fn local_name_string(qname: &[u8]) -> Result<String> {
    let local = match qname.iter().position(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    };
    Ok(std::str::from_utf8(local)?.to_string())
}

const INLINE_CONSTANT_ATTRS: &[&str] = &[
    "Binary",
    "Bool",
    "Date",
    "DateTimeOffset",
    "Decimal",
    "Duration",
    "EnumMember",
    "Float",
    "Guid",
    "Int",
    "String",
    "TimeOfDay",
    "AnnotationPath",
    "NavigationPropertyPath",
    "Path",
    "PropertyPath",
    "UrlRef",
];

fn find_inline_constant_idx(attrs: &[(String, String)]) -> Option<usize> {
    attrs
        .iter()
        .position(|(k, _)| INLINE_CONSTANT_ATTRS.contains(&k.as_str()))
}

fn constant_attr_to_expr(key: &str, value: String) -> CsdlAnnotationExpression {
    match key {
        "Binary" => CsdlAnnotationExpression::Binary(value.into_bytes()),
        "Bool" => CsdlAnnotationExpression::Bool(value.eq_ignore_ascii_case("true")),
        "Date" => CsdlAnnotationExpression::Date(value),
        "DateTimeOffset" => CsdlAnnotationExpression::DateTimeOffset(value),
        "Decimal" => CsdlAnnotationExpression::Decimal(value),
        "Duration" => CsdlAnnotationExpression::Duration(value),
        "EnumMember" => CsdlAnnotationExpression::EnumMember(value),
        "Float" => CsdlAnnotationExpression::Float(value.parse().unwrap_or(0.0)),
        "Guid" => CsdlAnnotationExpression::Guid(value),
        "Int" => CsdlAnnotationExpression::Int(value.parse().unwrap_or(0)),
        "String" => CsdlAnnotationExpression::String(value),
        "TimeOfDay" => CsdlAnnotationExpression::TimeOfDay(value),
        "AnnotationPath" => CsdlAnnotationExpression::AnnotationPath(value),
        "NavigationPropertyPath" => CsdlAnnotationExpression::NavigationPropertyPath(value),
        "Path" => CsdlAnnotationExpression::Path(value),
        "PropertyPath" => CsdlAnnotationExpression::PropertyPath(value),
        _ => CsdlAnnotationExpression::String(value),
    }
}
