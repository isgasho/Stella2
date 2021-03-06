use cgmath::prelude::*;
use cgmath::{
    num_traits::NumCast, AbsDiffEq, BaseFloat, BaseNum, Point2, Point3, UlpsEq, Vector2, Vector3,
};
use std::{fmt, ops::Add};

use super::{Average2, BoolArray, ElementWiseOp, ElementWisePartialOrd};

pub trait AxisAlignedBox<T>: Sized {
    type Point: EuclideanSpace
        + ElementWiseOp
        + ElementWisePartialOrd
        + Add<Self::Vector, Output = Self::Point>
        + Average2;
    type Vector: Clone;

    fn new(min: Self::Point, max: Self::Point) -> Self;
    fn with_size(min: Self::Point, size: Self::Vector) -> Self {
        Self::new(min, min + size)
    }

    fn min(&self) -> Self::Point;
    fn max(&self) -> Self::Point;

    fn mid(&self) -> Self::Point {
        self.min().average2(&self.max())
    }

    fn zero() -> Self;

    /// Return `true` if a point is inside a box.
    ///
    /// # Examples
    ///
    ///     use cggeom::{prelude::*, Box2};
    ///     use cgmath::Point2;
    ///
    ///     let b = Box2::new(
    ///         Point2::new(0.0, 0.0),
    ///         Point2::new(1.0, 1.0),
    ///     );
    ///     assert!(b.contains_point(&Point2::new(0.0, 0.0)));
    ///     assert!(b.contains_point(&Point2::new(0.5, 0.5)));
    ///
    ///     assert!(!b.contains_point(&Point2::new(0.5, -1.0)));
    ///     assert!(!b.contains_point(&Point2::new(0.5, 1.0)));
    ///     assert!(!b.contains_point(&Point2::new(-1.0, 0.5)));
    ///     assert!(!b.contains_point(&Point2::new(1.0, 0.5)));
    ///
    #[inline]
    fn contains_point(&self, point: &Self::Point) -> bool
    where
        T: PartialOrd,
    {
        point.element_wise_ge(&self.min()).all() && point.element_wise_lt(&self.max()).all()
    }

    /// Return `true` if a point is inside or on the boundary of a box.
    ///
    /// # Examples
    ///
    ///     use cggeom::{prelude::*, Box2};
    ///     use cgmath::Point2;
    ///
    ///     let b = Box2::new(
    ///         Point2::new(0.0, 0.0),
    ///         Point2::new(1.0, 1.0),
    ///     );
    ///     assert!(b.contains_point_incl(&Point2::new(0.0, 0.0)));
    ///     assert!(b.contains_point_incl(&Point2::new(0.5, 0.5)));
    ///     assert!(b.contains_point_incl(&Point2::new(0.5, 1.0)));
    ///     assert!(b.contains_point_incl(&Point2::new(1.0, 0.5)));
    ///
    ///     assert!(!b.contains_point_incl(&Point2::new(0.5, -1.0)));
    ///     assert!(!b.contains_point_incl(&Point2::new(0.5, 1.2)));
    ///     assert!(!b.contains_point_incl(&Point2::new(-1.0, 0.5)));
    ///     assert!(!b.contains_point_incl(&Point2::new(1.2, 0.5)));
    #[inline]
    fn contains_point_incl(&self, point: &Self::Point) -> bool
    where
        T: PartialOrd,
    {
        point.element_wise_ge(&self.min()).all() && point.element_wise_le(&self.max()).all()
    }

    /// Return `true` if `other` is entirely inside `self`.
    ///
    /// # Examples
    ///
    ///     use cggeom::{prelude::*, box2};
    ///
    ///     let b = box2! { min: [0.0, 0.0], max: [1.0, 1.0] };
    ///     assert!(b.contains_box(&box2!{ min: [0.2, 0.0], max: [1.0, 0.5] }));
    ///
    ///     assert!(!b.contains_box(&box2!{ min: [0.2, 0.0], max: [1.2, 0.5] }));
    ///     assert!(!b.contains_box(&box2!{ min: [1.2, 0.3], max: [1.5, 0.8] }));
    ///     assert!(!b.contains_box(&box2!{ min: [0.3, 1.2], max: [0.8, 1.5] }));
    ///     assert!(!b.contains_box(&box2!{ min: [0.3, -0.5], max: [0.8, 0.3] }));
    ///
    #[inline]
    fn contains_box(&self, other: &Self) -> bool
    where
        T: PartialOrd,
    {
        other.min().element_wise_ge(&self.min()).all()
            && other.max().element_wise_le(&self.max()).all()
    }

    /// Return the in-bound point closest to `p`.
    ///
    /// # Examples
    ///
    ///     use cggeom::{prelude::*, box2};
    ///     use cgmath::Point2;
    ///
    ///     let b = box2! { min: [0.0, 0.0], max: [1.0, 1.0] };
    ///
    ///     assert_eq!(b.limit_point(&Point2::new(0.2, 0.5)), Point2::new(0.2, 0.5));
    ///     assert_eq!(b.limit_point(&Point2::new(-0.3, 0.5)), Point2::new(0.0, 0.5));
    ///
    #[inline]
    fn limit_point(&self, p: &Self::Point) -> Self::Point
    where
        T: BaseNum,
    {
        p.element_wise_max(&self.min())
            .element_wise_min(&self.max())
    }

    /// Return `true` iff at least one of the box's dimensions is < 0.
    ///
    /// # Examples
    ///
    ///     use cggeom::{box2, prelude::*};
    ///
    ///     assert!(box2! { min: [0u32, 0], max: [10, 10] }.is_valid() == true);
    ///     assert!(box2! { min: [10u32, 0], max: [10, 10] }.is_valid() == true);
    ///     assert!(box2! { min: [10u32, 0], max: [5, 10] }.is_valid() == false);
    ///
    fn is_valid(&self) -> bool;

    /// Return `true` iff at least one of the box's dimensions is ≤ 0.
    ///
    /// # Examples
    ///
    ///     use cggeom::{box2, prelude::*};
    ///
    ///     assert!(box2! { min: [0u32, 0], max: [10, 10] }.is_empty() == false);
    ///     assert!(box2! { min: [10u32, 0], max: [10, 10] }.is_empty() == true);
    ///     assert!(box2! { min: [10u32, 0], max: [5, 10] }.is_empty() == true);
    ///
    fn is_empty(&self) -> bool;

    /// Get the dimensions of the box.
    ///
    /// The dimensions are calculated as `self.max() - self.min()`. This may
    /// panic on overflow in debug builds.
    #[inline]
    fn size(&self) -> <Self::Point as EuclideanSpace>::Diff
    where
        T: BaseNum,
    {
        self.max() - self.min()
    }

    #[inline]
    fn union(&self, other: &Self) -> Self
    where
        T: BaseNum,
    {
        Self::new(
            self.min().element_wise_min(&other.min()),
            self.max().element_wise_max(&other.max()),
        )
    }

    #[inline]
    fn union_assign(&mut self, other: &Self)
    where
        T: BaseNum,
    {
        *self = self.union(other);
    }

    #[inline]
    fn intersection(&self, other: &Self) -> Option<Self>
    where
        T: BaseNum,
    {
        let s = Self::new(
            self.min().element_wise_max(&other.min()),
            self.max().element_wise_min(&other.max()),
        );
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    #[inline]
    fn translate(&self, displacement: Self::Vector) -> Self {
        Self::new(self.min() + displacement.clone(), self.max() + displacement)
    }
}

/// Represents an axis-aligned 2D box.
#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Box2<T> {
    /// The minimum coordinate (inclusive).
    pub min: Point2<T>,

    /// The maximum coordinate (exclusive).
    pub max: Point2<T>,
}

/// Represents an axis-aligned 3D box.
#[repr(C)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Box3<T> {
    /// The minimum coordinate (inclusive).
    pub min: Point3<T>,

    /// The maximum coordinate (exclusive).
    pub max: Point3<T>,
}

impl<T> Box2<T> {
    #[inline]
    pub const fn new(min: Point2<T>, max: Point2<T>) -> Self {
        Self { min, max }
    }
}

impl<T> Box3<T> {
    #[inline]
    pub const fn new(min: Point3<T>, max: Point3<T>) -> Self {
        Self { min, max }
    }
}

impl<T: BaseNum + Average2> AxisAlignedBox<T> for Box2<T> {
    type Point = Point2<T>;
    type Vector = Vector2<T>;

    #[inline]
    fn new(min: Self::Point, max: Self::Point) -> Self {
        Self { min, max }
    }

    #[inline]
    fn is_valid(&self) -> bool {
        self.max.x >= self.min.x && self.max.y >= self.min.y
    }
    #[inline]
    fn is_empty(&self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }

    #[inline]
    fn zero() -> Self {
        Self::new(
            Point2::new(T::zero(), T::zero()),
            Point2::new(T::zero(), T::zero()),
        )
    }

    #[inline]
    fn min(&self) -> Self::Point {
        self.min
    }
    #[inline]
    fn max(&self) -> Self::Point {
        self.max
    }
}

impl<T: BaseNum + Average2> AxisAlignedBox<T> for Box3<T> {
    type Point = Point3<T>;
    type Vector = Vector3<T>;

    #[inline]
    fn new(min: Self::Point, max: Self::Point) -> Self {
        Self { min, max }
    }

    #[inline]
    fn is_valid(&self) -> bool {
        self.max.x >= self.min.x && self.max.y >= self.min.y && self.max.z >= self.min.z
    }
    #[inline]
    fn is_empty(&self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y || self.max.z <= self.min.z
    }

    #[inline]
    fn zero() -> Self {
        Self::new(
            Point3::new(T::zero(), T::zero(), T::zero()),
            Point3::new(T::zero(), T::zero(), T::zero()),
        )
    }

    #[inline]
    fn min(&self) -> Self::Point {
        self.min
    }
    #[inline]
    fn max(&self) -> Self::Point {
        self.max
    }
}

impl<S: NumCast + Copy> Box2<S> {
    /// Component-wise casting to another type
    #[inline]
    pub fn cast<T: NumCast>(&self) -> Option<Box2<T>> {
        let min = match self.min.cast() {
            Some(field) => field,
            None => return None,
        };
        let max = match self.max.cast() {
            Some(field) => field,
            None => return None,
        };
        Some(Box2 { min, max })
    }
}

impl<S: NumCast + Copy> Box3<S> {
    /// Component-wise casting to another type
    #[inline]
    pub fn cast<T: NumCast>(&self) -> Option<Box3<T>> {
        let min = match self.min.cast() {
            Some(field) => field,
            None => return None,
        };
        let max = match self.max.cast() {
            Some(field) => field,
            None => return None,
        };
        Some(Box3 { min, max })
    }
}

/// ImageMagick-like formatting
#[derive(Debug)]
pub struct DisplayIm<'a, T>(&'a T);

impl<S> Box2<S> {
    /// Get an object implementing `Display` for printing `self` with
    /// ImageMagick-like formatting.
    ///
    ///     use cggeom::box2;
    ///
    ///     let bx = box2!{ top_left: [-1i32, 2], size: [4, 8] };
    ///     let st = format!("{}", bx.display_im());
    ///     assert_eq!(st, "4x8-1+2");
    ///
    pub fn display_im(&self) -> DisplayIm<'_, Self> {
        DisplayIm(self)
    }
}

impl<S> Box3<S> {
    /// Get an object implementing `Display` for printing `self` with
    /// ImageMagick-like formatting.
    pub fn display_im(&self) -> DisplayIm<'_, Self> {
        DisplayIm(self)
    }
}

impl<T: BaseNum + Average2 + fmt::Display> fmt::Display for DisplayIm<'_, Box2<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = self.0.size();
        let min = self.0.min();
        write!(f, "{}x{}{:+}{:+}", size.x, size.y, min.x, min.y)
    }
}

/// Display dimensions in a ImageMagick-like format (`wxhxd+x+y+z`).
impl<T: BaseNum + Average2 + fmt::Display> fmt::Display for DisplayIm<'_, Box3<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = self.0.size();
        let min = self.0.min();
        write!(
            f,
            "{}x{}x{}{:+}{:+}{:+}",
            size.x, size.y, size.z, min.x, min.y, min.z
        )
    }
}

impl<S: BaseFloat> AbsDiffEq for Box2<S> {
    type Epsilon = S::Epsilon;

    fn default_epsilon() -> Self::Epsilon {
        S::default_epsilon()
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.min.abs_diff_eq(&other.min, epsilon) && self.max.abs_diff_eq(&other.max, epsilon)
    }

    fn abs_diff_ne(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.min.abs_diff_ne(&other.min, epsilon) || self.max.abs_diff_ne(&other.max, epsilon)
    }
}

impl<S: BaseFloat> AbsDiffEq for Box3<S> {
    type Epsilon = S::Epsilon;

    fn default_epsilon() -> Self::Epsilon {
        S::default_epsilon()
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.min.abs_diff_eq(&other.min, epsilon) && self.max.abs_diff_eq(&other.max, epsilon)
    }

    fn abs_diff_ne(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.min.abs_diff_ne(&other.min, epsilon) || self.max.abs_diff_ne(&other.max, epsilon)
    }
}

impl<S: BaseFloat> UlpsEq for Box2<S> {
    fn default_max_ulps() -> u32 {
        S::default_max_ulps()
    }

    fn ulps_eq(&self, other: &Self, epsilon: Self::Epsilon, max_ulps: u32) -> bool {
        self.min.ulps_eq(&other.min, epsilon, max_ulps)
            && self.max.ulps_eq(&other.max, epsilon, max_ulps)
    }

    fn ulps_ne(&self, other: &Self, epsilon: Self::Epsilon, max_ulps: u32) -> bool {
        self.min.ulps_ne(&other.min, epsilon, max_ulps)
            || self.max.ulps_ne(&other.max, epsilon, max_ulps)
    }
}

impl<S: BaseFloat> UlpsEq for Box3<S> {
    fn default_max_ulps() -> u32 {
        S::default_max_ulps()
    }

    fn ulps_eq(&self, other: &Self, epsilon: Self::Epsilon, max_ulps: u32) -> bool {
        self.min.ulps_eq(&other.min, epsilon, max_ulps)
            && self.max.ulps_eq(&other.max, epsilon, max_ulps)
    }

    fn ulps_ne(&self, other: &Self, epsilon: Self::Epsilon, max_ulps: u32) -> bool {
        self.min.ulps_ne(&other.min, epsilon, max_ulps)
            || self.max.ulps_ne(&other.max, epsilon, max_ulps)
    }
}

#[cfg(feature = "quickcheck")]
use quickcheck::{Arbitrary, Gen};

#[cfg(feature = "quickcheck")]
impl<T: Arbitrary + BaseNum + Average2> Arbitrary for Box2<T> {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let (x1, x2, x3, x4) = Arbitrary::arbitrary(g);
        Box2::new(Point2::new(x1, x2), Point2::new(x3, x4))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(
            (self.min.x, self.min.y, self.max.x, self.max.y)
                .shrink()
                .map(|(x1, x2, x3, x4)| Box2::new(Point2::new(x1, x2), Point2::new(x3, x4))),
        )
    }
}

#[cfg(feature = "quickcheck")]
impl<T: Arbitrary + BaseNum + Average2> Arbitrary for Box3<T> {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let (x1, x2, x3, x4, x5, x6) = Arbitrary::arbitrary(g);
        Box3::new(Point3::new(x1, x2, x3), Point3::new(x4, x5, x6))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(
            (
                self.min.x, self.min.y, self.min.z, self.max.x, self.max.y, self.max.z,
            )
                .shrink()
                .map(|(x1, x2, x3, x4, x5, x6)| {
                    Box3::new(Point3::new(x1, x2, x3), Point3::new(x4, x5, x6))
                }),
        )
    }
}

/// A macro for constructing `Box2` using various types of origin points.
///
/// When *all* endpoints and sizes are specified by `[x, y]`, they are
/// automatically converted to `Vector2` or `Point2`.
///
/// This macro is `const fn`-compatible.
///
/// The syntax of this macro assumes a coordinate space where the increases in
/// X and Y coordinates correspond to the right and down direction, respectively.
///
/// # Examples
///
/// ```
/// use {cggeom::{box2, Box2, prelude::*}, cgmath::Point2};
///
/// let ref_box = Box2::new(Point2::new(1, 2), Point2::new(5, 10));
///
/// assert_eq!(ref_box, box2!{ min: [1, 2], max: [5, 10] });
/// assert_eq!(ref_box, box2!{ top_left: [1, 2], size: [4, 8] });
/// assert_eq!(ref_box, box2!{ top_right: [5, 2], size: [4, 8] });
/// assert_eq!(ref_box, box2!{ bottom_left: [1, 10], size: [4, 8] });
/// assert_eq!(ref_box, box2!{ bottom_right: [5, 10], size: [4, 8] });
///
/// let ref_point = Box2::new(Point2::new(1, 2), Point2::new(1, 2));
///
/// assert_eq!(ref_point, box2!{ point: [1, 2] });
/// ```
#[macro_export]
macro_rules! box2 {
    {
        point: [$x:expr, $y:expr $(,)*]$(,)*
    } => {{
        let point = $crate::cgmath::Point2::new($x, $y);
        $crate::Box2::new(point, point)
    }};

    {
        point: $point:expr$(,)*
    } => {{
        let point = $point;
        $crate::Box2::new(point, point)
    }};

    {
        min: [$min_x:expr, $min_y:expr $(,)*],
        max: [$max_x:expr, $max_y:expr $(,)*]$(,)*
    } => {{
        let min = $crate::cgmath::Point2::new($min_x, $min_y);
        let max = $crate::cgmath::Point2::new($max_x, $max_y);
        $crate::Box2::new(min, max)
    }};

    {
        min: $min:expr,
        max: $max:expr$(,)*
    } => {
        $crate::Box2::new($min, $max)
    };

    {
        top_left: [$origin_x:expr, $origin_y:expr $(,)*],
        size: [$size_x:expr, $size_y:expr $(,)*]$(,)*
    } => {{
        let origin = $crate::cgmath::Point2::new($origin_x, $origin_y);
        let size = $crate::cgmath::Vector2::new($size_x, $size_y);
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x, origin.y),
            $crate::cgmath::Point2::new(origin.x + size.x, origin.y + size.y),
        )
    }};

    {
        top_left: $origin:expr,
        size: $size:expr$(,)*
    } => {{
        let origin: $crate::cgmath::Point2<_> = $origin;
        let size: $crate::cgmath::Vector2<_> = $size;
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x, origin.y),
            $crate::cgmath::Point2::new(origin.x + size.x, origin.y + size.y),
        )
    }};

    {
        top_right: [$origin_x:expr, $origin_y:expr $(,)*],
        size: [$size_x:expr, $size_y:expr $(,)*]$(,)*
    } => {{
        let origin = $crate::cgmath::Point2::new($origin_x, $origin_y);
        let size = $crate::cgmath::Vector2::new($size_x, $size_y);
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x - size.x, origin.y),
            $crate::cgmath::Point2::new(origin.x, origin.y + size.y),
        )
    }};

    {
        top_right: $origin:expr,
        size: $size:expr$(,)*
    } => {{
        let origin: $crate::cgmath::Point2<_> = $origin;
        let size: $crate::cgmath::Vector2<_> = $size;
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x - size.x, origin.y),
            $crate::cgmath::Point2::new(origin.x, origin.y + size.y),
        )
    }};

    {
        bottom_left: [$origin_x:expr, $origin_y:expr $(,)*],
        size: [$size_x:expr, $size_y:expr $(,)*]$(,)*
    } => {{
        let origin = $crate::cgmath::Point2::new($origin_x, $origin_y);
        let size = $crate::cgmath::Vector2::new($size_x, $size_y);
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x, origin.y - size.y),
            $crate::cgmath::Point2::new(origin.x + size.x, origin.y),
        )
    }};

    {
        bottom_left: $origin:expr,
        size: $size:expr$(,)*
    } => {{
        let origin: $crate::cgmath::Point2<_> = $origin;
        let size: $crate::cgmath::Vector2<_> = $size;
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x, origin.y - size.y),
            $crate::cgmath::Point2::new(origin.x + size.x, origin.y),
        )
    }};

    {
        bottom_right: [$origin_x:expr, $origin_y:expr $(,)*],
        size: [$size_x:expr, $size_y:expr $(,)*]$(,)*
    } => {{
        let origin = $crate::cgmath::Point2::new($origin_x, $origin_y);
        let size = $crate::cgmath::Vector2::new($size_x, $size_y);
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x - size.x, origin.y - size.y),
            $crate::cgmath::Point2::new(origin.x, origin.y),
        )
    }};

    {
        bottom_right: $origin:expr,
        size: $size:expr$(,)*
    } => {{
        let origin: $crate::cgmath::Point2<_> = $origin;
        let size: $crate::cgmath::Vector2<_> = $size;
        $crate::Box2::new(
            $crate::cgmath::Point2::new(origin.x - size.x, origin.y - size.y),
            $crate::cgmath::Point2::new(origin.x, origin.y),
        )
    }}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_empty_box2_u32() {
        let bx: Box2<u32> = Box2::new([20, 20].into(), [30, 30].into());
        assert!(!bx.is_empty(), "{:?}", bx);

        let bxs: &[Box2<u32>] = &[
            Box2::new([20, 20].into(), [20, 30].into()),
            Box2::new([20, 20].into(), [15, 30].into()),
            Box2::new([20, 20].into(), [30, 20].into()),
            Box2::new([20, 20].into(), [30, 15].into()),
            Box2::new([20, 20].into(), [20, 20].into()),
            Box2::new([20, 20].into(), [15, 15].into()),
        ];

        for bx in bxs {
            assert!(bx.is_empty(), "{:?}", bx);
        }
    }

    #[test]
    fn is_empty_box3_u32() {
        let bx: Box3<u32> = Box3::new([20, 20, 20].into(), [30, 30, 30].into());
        assert!(!bx.is_empty(), "{:?}", bx);

        let bxs: &[Box3<u32>] = &[
            Box3::new([20, 20, 20].into(), [20, 30, 20].into()),
            Box3::new([20, 20, 20].into(), [15, 30, 20].into()),
            Box3::new([20, 20, 20].into(), [30, 20, 20].into()),
            Box3::new([20, 20, 20].into(), [30, 15, 20].into()),
            Box3::new([20, 20, 20].into(), [20, 20, 20].into()),
            Box3::new([20, 20, 20].into(), [15, 15, 20].into()),
            Box3::new([20, 20, 20].into(), [30, 20, 20].into()),
            Box3::new([20, 20, 20].into(), [30, 20, 15].into()),
            Box3::new([20, 20, 20].into(), [20, 20, 30].into()),
        ];

        for bx in bxs {
            assert!(bx.is_empty(), "{:?}", bx);
        }
    }

    #[test]
    fn is_valid_box2_u32() {
        let bxs: &[Box2<u32>] = &[
            Box2::new([20, 20].into(), [30, 30].into()),
            Box2::new([20, 20].into(), [20, 30].into()),
            Box2::new([20, 20].into(), [30, 20].into()),
            Box2::new([20, 20].into(), [20, 20].into()),
        ];

        for bx in bxs {
            assert!(bx.is_valid(), "{:?}", bx);
        }

        let bxs: &[Box2<u32>] = &[
            Box2::new([20, 20].into(), [15, 30].into()),
            Box2::new([20, 20].into(), [30, 15].into()),
            Box2::new([20, 20].into(), [15, 15].into()),
        ];

        for bx in bxs {
            assert!(!bx.is_valid(), "{:?}", bx);
        }
    }

    #[test]
    fn is_valid_box3_u32() {
        let bxs: &[Box3<u32>] = &[
            Box3::new([20, 20, 20].into(), [30, 30, 30].into()),
            Box3::new([20, 20, 20].into(), [20, 30, 30].into()),
            Box3::new([20, 20, 20].into(), [30, 20, 20].into()),
            Box3::new([20, 20, 20].into(), [20, 20, 20].into()),
        ];

        for bx in bxs {
            assert!(bx.is_valid(), "{:?}", bx);
        }

        let bxs: &[Box3<u32>] = &[
            Box3::new([20, 20, 20].into(), [15, 30, 30].into()),
            Box3::new([20, 20, 20].into(), [30, 15, 30].into()),
            Box3::new([20, 20, 20].into(), [15, 15, 30].into()),
            Box3::new([20, 20, 20].into(), [30, 30, 15].into()),
        ];

        for bx in bxs {
            assert!(!bx.is_valid(), "{:?}", bx);
        }
    }
}
