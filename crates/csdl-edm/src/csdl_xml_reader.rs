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
                        if let Some(idx) = find_inline_constant_idx(&attrs)? {
                            let (k, v) = attrs.swap_remove(idx);
                            exprs.push(constant_attr_to_expr(&k, v)?);
                        }
                        let (body, annotations) = self.read_expression_content("Annotation")?;
                        exprs.extend(body);
                        if exprs.len() > 1 {
                            return Err(
                                self.csdl_err("Annotation allows only one value expression")
                            );
                        }
                        for ex in exprs {
                            self.deferred
                                .push_back(SyntaxUnit::AnnotationExpression(ex));
                        }
                        for annotation in annotations {
                            self.queue_annotation(annotation);
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
                        if let Some(idx) = find_inline_constant_idx(&attrs)? {
                            let (k, v) = attrs.swap_remove(idx);
                            self.deferred.push_back(SyntaxUnit::AnnotationExpression(
                                constant_attr_to_expr(&k, v)?,
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
        use CsdlAnnotationExpression as E;
        if (INLINE_CONSTANT_ATTRS.contains(&name) && name != "UrlRef")
            || name == "LabeledElementReference"
        {
            return constant_attr_to_expr(name, self.read_text_body(name)?);
        }
        if name == "Record" {
            let (properties, annotations) = self.read_record_body(name)?;
            return Ok(E::Record {
                type_: attr_value(&attrs, "Type"),
                properties,
                annotations,
            });
        }
        let (mut children, annotations) = self.read_expression_content(name)?;
        if name == "LabeledElement" {
            if let Some(idx) = find_inline_constant_idx(&attrs)? {
                children.push(constant_attr_to_expr(&attrs[idx].0, attrs[idx].1.clone())?);
            }
        }
        let mut expression = match name {
            "Not" | "Neg" | "UrlRef" => {
                let [child] = exact::<1>(name, children)?;
                match name {
                    "Not" => E::Not(Box::new(child)),
                    "Neg" => E::Neg(Box::new(child)),
                    _ => E::UrlRef(Box::new(child)),
                }
            }
            n if BinaryOperator::parse(n).is_some() => {
                let [lhs, rhs] = exact::<2>(name, children)?;
                E::BinaryExpression {
                    op: BinaryOperator::parse(name).unwrap(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                }
            }
            "If" => {
                if !(2..=3).contains(&children.len()) {
                    return Err(self.csdl_err("If requires two or three operands"));
                }
                let mut children = children.into_iter();
                E::If {
                    test: Box::new(children.next().unwrap()),
                    then_: Box::new(children.next().unwrap()),
                    else_: children.next().map(Box::new),
                }
            }
            "Apply" => E::Apply {
                function: required_attr(&attrs, "Function")?,
                args: children,
            },
            "Cast" | "IsOf" => {
                let [expr] = exact::<1>(name, children)?;
                let type_ = required_attr(&attrs, "Type")?;
                let base = if name == "Cast" {
                    E::Cast {
                        type_,
                        expr: Box::new(expr),
                    }
                } else {
                    E::IsOf {
                        type_,
                        expr: Box::new(expr),
                    }
                };
                let facets = crate::expr::ExpressionFacets::from_attributes(&attrs)?;
                if facets == Default::default() {
                    base
                } else {
                    E::Faceted {
                        expression: Box::new(base),
                        facets,
                    }
                }
            }
            "LabeledElement" => {
                let [expr] = exact::<1>(name, children)?;
                E::LabeledElement {
                    name: required_attr(&attrs, "Name")?,
                    expr: Box::new(expr),
                }
            }
            "Collection" => {
                if !annotations.is_empty() {
                    return Err(self.csdl_err("Collection cannot contain Annotation children"));
                }
                E::Collection(children)
            }
            "Null" => {
                exact::<0>(name, children)?;
                E::Null
            }
            _ => return Err(self.csdl_err(format!("Unknown expression: {name}"))),
        };
        if !annotations.is_empty() {
            expression = E::Annotated {
                expression: Box::new(expression),
                annotations,
            };
        }
        Ok(expression)
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
    fn read_expression_content(
        &mut self,
        parent: &str,
    ) -> Result<(Vec<CsdlAnnotationExpression>, Vec<Annotation>)> {
        let mut out = Vec::new();
        let mut annotations = Vec::new();
        loop {
            self.buf.clear();
            match self.inner.read_event_into(&mut self.buf)? {
                Event::Start(e) => {
                    let (n, mut a) = parse_start(&e)?;
                    if n == "Annotation" {
                        annotations.push(self.read_annotation_start(&mut a)?);
                    } else {
                        out.push(self.build_expr_from_start(&n, a)?);
                    }
                }
                Event::Empty(e) => {
                    let (n, mut a) = parse_start(&e)?;
                    if n == "Annotation" {
                        annotations.push(self.read_annotation_empty(&mut a)?);
                    } else {
                        out.push(build_expr_from_empty(&n, a)?);
                    }
                }
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == parent {
                        return Ok((out, annotations));
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
                            let value = find_inline_constant_idx(&a)?
                                .map(|idx| {
                                    let (k, v) = a.swap_remove(idx);
                                    constant_attr_to_expr(&k, v)
                                })
                                .transpose()?;
                            properties.push(PropertyValue {
                                property,
                                value,
                                annotations: Vec::new(),
                            });
                        }
                        "Annotation" => {
                            annotations.push(self.read_annotation_empty(&mut a)?);
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
        let inline = find_inline_constant_idx(attrs)?
            .map(|idx| {
                let (k, v) = attrs.swap_remove(idx);
                constant_attr_to_expr(&k, v)
            })
            .transpose()?;
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
                        annotations.push(self.read_annotation_empty(&mut a)?);
                    } else {
                        body_values.push(build_expr_from_empty(&n, a)?);
                    }
                }
                Event::End(e) => {
                    let n = local_name_string(e.name().as_ref())?;
                    if n == "PropertyValue" {
                        let value = pick_first_value(inline, body_values)?;
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
        let inline = find_inline_constant_idx(attrs)?
            .map(|idx| {
                let (k, v) = attrs.swap_remove(idx);
                constant_attr_to_expr(&k, v)
            })
            .transpose()?;
        let (body, annotations) = self.read_expression_content("Annotation")?;
        let expression = pick_first_value(inline, body)?;
        Ok(Annotation {
            annotations,
            term,
            qualifier,
            target,
            expression,
        })
    }

    /// Empty-element form (`<Annotation Term=".."/>`). Marker form
    /// (no inline constant attr) preserves the absence of an expression.
    fn read_annotation_empty(&mut self, attrs: &mut Vec<(String, String)>) -> Result<Annotation> {
        let term = attr_value(attrs, "Term").unwrap_or_default();
        let qualifier = attr_value(attrs, "Qualifier");
        let target = attr_value(attrs, "Target");
        let expression = match find_inline_constant_idx(attrs)? {
            Some(idx) => {
                let (k, v) = attrs.swap_remove(idx);
                Some(constant_attr_to_expr(&k, v)?)
            }
            None => None,
        };
        Ok(Annotation {
            annotations: Vec::new(),
            term,
            qualifier,
            target,
            expression,
        })
    }

    fn queue_annotation(&mut self, annotation: Annotation) {
        let mut attributes = vec![("Term".into(), annotation.term)];
        if let Some(q) = annotation.qualifier {
            attributes.push(("Qualifier".into(), q));
        }
        if let Some(t) = annotation.target {
            attributes.push(("Target".into(), t));
        }
        self.deferred.push_back(SyntaxUnit::StartElement {
            name: "Annotation".into(),
            attributes,
        });
        if let Some(e) = annotation.expression {
            self.deferred.push_back(SyntaxUnit::AnnotationExpression(e));
        }
        for annotation in annotation.annotations {
            self.queue_annotation(annotation);
        }
        self.deferred.push_back(SyntaxUnit::EndElement {
            name: "Annotation".into(),
        });
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
) -> Result<Option<CsdlAnnotationExpression>> {
    if body.len() + usize::from(inline.is_some()) > 1 {
        return Err(Error::Csdl(
            "Only one annotation value expression is allowed".into(),
        ));
    }
    Ok(match inline {
        Some(v) => Some(v),
        None if body.is_empty() => None,
        None => Some(body.swap_remove(0)),
    })
}

fn exact<const N: usize>(
    name: &str,
    v: Vec<CsdlAnnotationExpression>,
) -> Result<[CsdlAnnotationExpression; N]> {
    let got = v.len();
    v.try_into()
        .map_err(|_| Error::Csdl(format!("<{name}> expects {N} children, got {got}")))
}

fn required_attr(attrs: &[(String, String)], key: &str) -> Result<String> {
    attr_value(attrs, key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Csdl(format!("Expression requires {key}")))
}

fn build_expr_from_empty(
    name: &str,
    attrs: Vec<(String, String)>,
) -> Result<CsdlAnnotationExpression> {
    use CsdlAnnotationExpression as E;
    Ok(match name {
        "Null" => E::Null,
        "String" => E::String(String::new()),
        "Binary" => E::Binary(Vec::new()),
        "Collection" => E::Collection(Vec::new()),
        "Record" => E::Record {
            type_: attr_value(&attrs, "Type"),
            properties: vec![],
            annotations: vec![],
        },
        "Apply" => E::Apply {
            function: required_attr(&attrs, "Function")?,
            args: vec![],
        },
        "LabeledElement" => {
            let idx = find_inline_constant_idx(&attrs)?
                .ok_or_else(|| Error::Csdl("LabeledElement requires a value".into()))?;
            E::LabeledElement {
                name: required_attr(&attrs, "Name")?,
                expr: Box::new(constant_attr_to_expr(&attrs[idx].0, attrs[idx].1.clone())?),
            }
        }
        _ => return Err(Error::Csdl(format!("Empty {name} expression is invalid"))),
    })
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
    "ModelElementPath",
    "NavigationPropertyPath",
    "Path",
    "PropertyPath",
    "UrlRef",
];

fn find_inline_constant_idx(attrs: &[(String, String)]) -> Result<Option<usize>> {
    let indices = attrs
        .iter()
        .enumerate()
        .filter(|(_, (k, _))| INLINE_CONSTANT_ATTRS.contains(&k.as_str()))
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    if indices.len() > 1 {
        return Err(Error::Csdl("Only one inline expression is allowed".into()));
    }
    Ok(indices.first().copied())
}

fn constant_attr_to_expr(key: &str, value: String) -> Result<CsdlAnnotationExpression> {
    Ok(match key {
        "Binary" => CsdlAnnotationExpression::Binary(value.into_bytes()),
        "Bool" => CsdlAnnotationExpression::Bool(match value.as_str() {
            "true" => true,
            "false" => false,
            _ => return Err(Error::Csdl(format!("invalid Bool constant: {value}"))),
        }),
        "Date" => CsdlAnnotationExpression::Date(value),
        "DateTimeOffset" => CsdlAnnotationExpression::DateTimeOffset(value),
        "Decimal" => CsdlAnnotationExpression::Decimal(value),
        "Duration" => CsdlAnnotationExpression::Duration(value),
        "EnumMember" => CsdlAnnotationExpression::EnumMember(value),
        "Float" => CsdlAnnotationExpression::Float(
            value
                .parse()
                .map_err(|_| Error::Csdl(format!("invalid Float constant: {value}")))?,
        ),
        "Guid" => CsdlAnnotationExpression::Guid(value),
        "Int" => CsdlAnnotationExpression::Int(
            value
                .parse()
                .map_err(|_| Error::Csdl(format!("invalid Int constant: {value}")))?,
        ),
        "String" => CsdlAnnotationExpression::String(value),
        "TimeOfDay" => CsdlAnnotationExpression::TimeOfDay(value),
        "AnnotationPath" => CsdlAnnotationExpression::AnnotationPath(value),
        "ModelElementPath" => CsdlAnnotationExpression::ModelElementPath(value),
        "LabeledElementReference" => CsdlAnnotationExpression::LabeledElementReference(value),
        "UrlRef" => {
            CsdlAnnotationExpression::UrlRef(Box::new(CsdlAnnotationExpression::String(value)))
        }
        "NavigationPropertyPath" => CsdlAnnotationExpression::NavigationPropertyPath(value),
        "Path" => CsdlAnnotationExpression::Path(value),
        "PropertyPath" => CsdlAnnotationExpression::PropertyPath(value),
        _ => CsdlAnnotationExpression::String(value),
    })
}
