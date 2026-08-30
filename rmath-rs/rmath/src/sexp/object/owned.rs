use super::super::ffi::{NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, SEXPTYPE};
use super::pairlist::PairlistIter;
use super::value::{
    OWNED_VALUE_ATTRIBUTE_DEPTH_LIMIT, SexpAttribute, SexpComplex, SexpMetadata, SexpValue,
    sexptype_name,
};
use super::{Sexp, SexpResult};

impl<'a> Sexp<'a> {
    /// Convert this SEXP into an owned Rust value.
    ///
    /// Scalar logical, integer, and real vectors of length one become scalar
    /// variants. Other atomic vectors remain vectors. Missing R values are
    /// represented as `None`, and nested generic/expression vectors are
    /// recursively projected.
    // Owning receiver is intentional: projection consumes the handle
    // (non-Copy by design); the value itself is cloned internally as needed.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_owned_value(self) -> SexpResult<SexpValue> {
        self.to_owned_value_inner(0)
    }

    // Owning receiver is intentional: projection consumes the handle
    // (non-Copy by design); the value itself is cloned internally as needed.
    #[allow(clippy::wrong_self_convention)]
    pub(super) fn to_owned_value_inner(self, depth: usize) -> SexpResult<SexpValue> {
        let value = self.clone().to_owned_value_without_attributes(depth).clone()?;
        if depth >= OWNED_VALUE_ATTRIBUTE_DEPTH_LIMIT {
            return Ok(value);
        }

        let Some(metadata) = self.to_owned_metadata(depth)? else {
            return Ok(value);
        };

        Ok(SexpValue::Attributed {
            value: Box::new(value),
            metadata,
        })
    }

    // Owning receiver is intentional: projection consumes the handle
    // (non-Copy by design); the value itself is cloned internally as needed.
    #[allow(clippy::wrong_self_convention)]
    fn to_owned_value_without_attributes(self, depth: usize) -> SexpResult<SexpValue> {
        let len = self.clone().len();
        match self.clone().typeof_(){
            SEXPTYPE::NILSXP => Ok(SexpValue::Null),
            SEXPTYPE::LGLSXP => {
                let values = self.try_logical_values()?;
                if len == 1 {
                    Ok(SexpValue::Logical(values.into_iter().next().flatten()))
                } else {
                    Ok(SexpValue::LogicalVector(values))
                }
            }
            SEXPTYPE::INTSXP => {
                let values = self.try_integer_values()?;
                if len == 1 {
                    Ok(SexpValue::Integer(values.into_iter().next().flatten()))
                } else {
                    Ok(SexpValue::IntegerVector(values))
                }
            }
            SEXPTYPE::REALSXP => {
                let values = self.try_real_values()?;
                if len == 1 {
                    Ok(SexpValue::Real(values.into_iter().next().flatten()))
                } else {
                    Ok(SexpValue::RealVector(values))
                }
            }
            SEXPTYPE::STRSXP => self.try_string_values().map(SexpValue::StringVector),
            SEXPTYPE::RAWSXP => self
                .try_as_raw_slice()
                .map(|values| SexpValue::RawVector(values.to_vec())),
            SEXPTYPE::CPLXSXP => self.try_complex_values().map(SexpValue::ComplexVector),
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                let mut values = Vec::with_capacity(len as usize);
                for i in 0..len {
                    values.push(self.clone().try_vector_elt(i)?.to_owned_value_inner(depth + 1)?);
                }
                Ok(SexpValue::List(values))
            }
            _ => Ok(SexpValue::Unsupported {
                type_name: sexptype_name(self.typeof_()).to_string(),
            }),
        }
    }

    // Owning receiver is intentional: projection consumes the handle
    // (non-Copy by design); the value itself is cloned internally as needed.
    #[allow(clippy::wrong_self_convention)]
    fn to_owned_metadata(self, depth: usize) -> SexpResult<Option<SexpMetadata>> {
        let Some(attrib) = self.attrib() else {
            return Ok(None);
        };
        if attrib.clone().is_nil() {
            return Ok(None);
        }

        let mut metadata = SexpMetadata::default();
        for cell in PairlistIter::new(attrib) {
            let Some(name) = cell.clone().attribute_name().clone()? else {
                continue;
            };
            let value = cell.try_car()?;

            match name.as_str() {
                "names" => metadata.names = value.clone().try_string_values().clone().ok(),
                "dim" => {
                    metadata.dim = value
                        .clone().try_integer_values().clone().ok()
                        .and_then(|values| values.into_iter().collect());
                }
                "class" => metadata.class = value.clone().try_string_values().clone().ok(),
                "levels" => metadata.levels = value.clone().try_string_values().clone().ok(),
                _ => {}
            }

            metadata.attributes.push(SexpAttribute {
                name,
                value: value.to_owned_value_inner(depth + 1)?,
            });
        }

        if metadata.attributes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(metadata))
        }
    }

    fn attribute_name(self) -> SexpResult<Option<String>> {
        let tag = self.try_tag()?;
        if tag.clone().is_nil() {
            return Ok(None);
        }
        if tag.clone().typeof_()!= SEXPTYPE::SYMSXP {
            return Ok(None);
        }

        Ok(Some(tag.try_printname()?.try_as_str()?.to_string()))
    }

    // Ownership: element accessors consume the handle, so each element read
    // clones the handle; the clones alias the same R object (no deep copy).
    fn try_logical_values(self) -> SexpResult<Vec<Option<bool>>> {
        let len = self.clone().len();
        (0..len)
            .map(|i| {
                self.clone().try_logical_elt(i).map(|value| match value {
                    NA_LOGICAL => None,
                    0 => Some(false),
                    _ => Some(true),
                })
            })
            .collect()
    }

    fn try_integer_values(self) -> SexpResult<Vec<Option<i32>>> {
        let len = self.clone().len();
        (0..len)
            .map(|i| {
                self.clone().try_integer_elt(i).map(|value| {
                    if value == NA_INTEGER {
                        None
                    } else {
                        Some(value)
                    }
                })
            })
            .collect()
    }

    fn try_real_values(self) -> SexpResult<Vec<Option<f64>>> {
        let len = self.clone().len();
        (0..len)
            .map(|i| {
                self.clone().try_real_elt(i).map(|value| {
                    if value.to_bits() == R_NA_BIT_PATTERN {
                        None
                    } else {
                        Some(value)
                    }
                })
            })
            .collect()
    }

    fn try_string_values(self) -> SexpResult<Vec<Option<String>>> {
        let len = self.clone().len();
        (0..len)
            .map(|i| {
                self.clone().try_string_text_elt(i)
                    .map(|value| value.map(str::to_string))
            })
            .collect()
    }

    fn try_complex_values(self) -> SexpResult<Vec<Option<SexpComplex>>> {
        let len = self.clone().len();
        (0..len)
            .map(|i| {
                self.clone().try_complex_elt(i).map(|value| {
                    if value.r.to_bits() == R_NA_BIT_PATTERN
                        || value.i.to_bits() == R_NA_BIT_PATTERN
                    {
                        None
                    } else {
                        Some(SexpComplex {
                            real: value.r,
                            imaginary: value.i,
                        })
                    }
                })
            })
            .collect()
    }
}
