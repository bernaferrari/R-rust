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
    pub fn to_owned_value(self) -> SexpResult<SexpValue> {
        self.to_owned_value_inner(0)
    }

    pub(super) fn to_owned_value_inner(self, depth: usize) -> SexpResult<SexpValue> {
        let value = self.to_owned_value_without_attributes(depth)?;
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

    fn to_owned_value_without_attributes(self, depth: usize) -> SexpResult<SexpValue> {
        let len = self.len();
        match self.typeof_() {
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
                    values.push(self.try_vector_elt(i)?.to_owned_value_inner(depth + 1)?);
                }
                Ok(SexpValue::List(values))
            }
            _ => Ok(SexpValue::Unsupported {
                type_name: sexptype_name(self.typeof_()).to_string(),
            }),
        }
    }

    fn to_owned_metadata(self, depth: usize) -> SexpResult<Option<SexpMetadata>> {
        let Some(attrib) = self.attrib() else {
            return Ok(None);
        };
        if attrib.is_nil() {
            return Ok(None);
        }

        let mut metadata = SexpMetadata::default();
        for cell in PairlistIter::new(attrib) {
            let Some(name) = cell.attribute_name()? else {
                continue;
            };
            let value = cell.try_car()?;

            match name.as_str() {
                "names" => metadata.names = value.try_string_values().ok(),
                "dim" => {
                    metadata.dim = value
                        .try_integer_values()
                        .ok()
                        .and_then(|values| values.into_iter().collect());
                }
                "class" => metadata.class = value.try_string_values().ok(),
                "levels" => metadata.levels = value.try_string_values().ok(),
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
        if tag.is_nil() {
            return Ok(None);
        }
        if tag.typeof_() != SEXPTYPE::SYMSXP {
            return Ok(None);
        }

        Ok(Some(tag.try_printname()?.try_as_str()?.to_string()))
    }

    fn try_logical_values(self) -> SexpResult<Vec<Option<bool>>> {
        (0..self.len())
            .map(|i| {
                self.try_logical_elt(i).map(|value| match value {
                    NA_LOGICAL => None,
                    0 => Some(false),
                    _ => Some(true),
                })
            })
            .collect()
    }

    fn try_integer_values(self) -> SexpResult<Vec<Option<i32>>> {
        (0..self.len())
            .map(|i| {
                self.try_integer_elt(i).map(|value| {
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
        (0..self.len())
            .map(|i| {
                self.try_real_elt(i).map(|value| {
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
        (0..self.len())
            .map(|i| {
                self.try_string_text_elt(i)
                    .map(|value| value.map(str::to_string))
            })
            .collect()
    }

    fn try_complex_values(self) -> SexpResult<Vec<Option<SexpComplex>>> {
        (0..self.len())
            .map(|i| {
                self.try_complex_elt(i).map(|value| {
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
