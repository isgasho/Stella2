//! Code generation for bit set types
use super::DisplayFn;

#[derive(Debug, Clone, Copy)]
pub struct TooLargeError;

#[allow(dead_code)] // Work-around rust-lang/rust#64362
#[derive(Debug, Clone, Copy)]
pub enum BitsetTy {
    Empty,
    Ux { ty: &'static str },
}

impl BitsetTy {
    /// Construct a code generator for a bitset type capable of storing `size`
    /// elements.
    pub fn new(size: usize) -> Result<Self, TooLargeError> {
        // A few points regarding this branch:
        //  - `u8` should be more than sufficient for most use cases.
        //  - `u16` and `u64` often need a size prefix, so it's beneficial in terms
        //    of code size to use `u32` in lieu of `u64`.
        //  - If we use the same type for all components, the code will be slightly
        //    more amenable to compression (done by zswap or when distributing the
        //    application)
        //  - `u128` is not natively supported by x86, so it generates code that is
        //    larger and slower (by like a few nanoseconds).
        if size == 0 {
            Ok(Self::Empty)
        } else if size <= 8 {
            Ok(Self::Ux { ty: "u8" })
        } else if size <= 32 {
            Ok(Self::Ux { ty: "u32" })
        } else if size <= 64 {
            Ok(Self::Ux { ty: "u64" })
        } else if size <= 128 {
            Ok(Self::Ux { ty: "u128" })
        } else {
            // Maybe we support a larger bitset in the future
            Err(TooLargeError)
        }
    }

    /// Get a `Display`-able type name.
    pub fn gen_ty(&self) -> impl std::fmt::Display + '_ {
        match self {
            Self::Empty => "()",
            Self::Ux { ty } => *ty,
        }
    }

    /// Generate an expression representing an empty set.
    pub fn gen_empty<'a>(&'a self) -> impl std::fmt::Display + 'a {
        DisplayFn(move |f| match self {
            Self::Empty => write!(f, "()"),
            Self::Ux { ty } => write!(f, "0{}", ty),
        })
    }

    /// Generate an expression that evaluates to a `bool` value indicating
    /// whether `expr` represents an empty set or not.
    pub fn gen_is_empty<'a>(
        &'a self,
        expr: impl std::fmt::Display + 'a,
    ) -> impl std::fmt::Display + 'a {
        DisplayFn(move |f| match self {
            Self::Empty => write!(f, "true"),
            Self::Ux { .. } => write!(f, "{} == 0", expr),
        })
    }

    /// Generate an expression that evaluates to a `bool` value indicating
    /// whether `expr` includes `i` as its element.
    pub fn gen_has<'a>(
        &'a self,
        expr: impl std::fmt::Display + 'a,
        i: usize,
    ) -> impl std::fmt::Display + 'a {
        self.gen_intersects(expr, Some(i))
    }

    /// Generate an expression that evaluates to a `bool` value indicating
    /// whether `expr` includes any of `elements` as its element.
    pub fn gen_intersects<'a>(
        &'a self,
        expr: impl std::fmt::Display + 'a,
        elements: impl IntoIterator<Item = usize> + Clone + 'a,
    ) -> impl std::fmt::Display + 'a {
        DisplayFn(move |f| match self {
            Self::Empty => write!(f, "false"),
            Self::Ux { .. } => write!(f, "({} & {}) != 0", expr, self.gen_multi(elements.clone())),
        })
    }

    /// Generate an expression that inserts specified elements to `expr`, and
    /// evaluates to `()`.
    pub fn gen_insert<'a>(
        &'a self,
        expr: impl std::fmt::Display + 'a,
        elements: impl IntoIterator<Item = usize> + Clone + 'a,
    ) -> impl std::fmt::Display + 'a {
        DisplayFn(move |f| match self {
            Self::Empty => panic!("vector size is 0, can't insert any elements"),
            Self::Ux { .. } => write!(f, "{} |= {}", expr, self.gen_multi(elements.clone())),
        })
    }

    /// Generate an expression that evaluates the union of `expr1` and `expr2`.
    /// `expr1` and `expr2` may or may not be evaluated.
    pub fn gen_union<'a>(
        &'a self,
        expr1: impl std::fmt::Display + 'a,
        expr2: impl std::fmt::Display + 'a,
    ) -> impl std::fmt::Display + 'a {
        DisplayFn(move |f| match self {
            Self::Empty => write!(f, "()"),
            Self::Ux { .. } => write!(f, "{} | {}", expr1, expr2),
        })
    }

    /// Generate an expression representing the specified set.
    pub fn gen_multi<'a>(
        &'a self,
        elements: impl IntoIterator<Item = usize> + Clone + 'a,
    ) -> impl std::fmt::Display + 'a {
        let single = {
            let mut elements = elements.clone().into_iter().fuse();
            let x = elements.next();
            if elements.next().is_some() {
                None
            } else {
                Some(x)
            }
        };

        DisplayFn(move |f| {
            if let Some(x) = single {
                if let Some(x) = x {
                    write!(f, "{}", self.gen_one(x))
                } else {
                    write!(f, "{}", self.gen_empty())
                }
            } else {
                // will panic anyway if `self` is `Self::Empty`
                let mut elements = elements.clone().into_iter();
                write!(f, "({})", self.gen_one(elements.next().unwrap()))?;
                for x in elements {
                    write!(f, " | ({})", self.gen_one(x))?;
                }
                Ok(())
            }
        })
    }

    /// Generate an expression representing the set `{i}`.
    fn gen_one(&self, i: usize) -> impl std::fmt::Display + '_ {
        DisplayFn(move |f| match self {
            Self::Empty => panic!("vector size is 0, can't have any elements"),
            Self::Ux { ty } => {
                if i == 0 {
                    write!(f, "1{}", ty)
                } else {
                    write!(f, "1{} << {}", ty, i)
                }
            }
        })
    }
}
