// This fixture triggers file-comments diagnostics.

/// Adds two numbers.
///
/// ```
/// let sum = add(1, 2);
/// assert_eq!(sum, 3);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

// A plain comment run between items, where inline-comments cannot see it.
// It keeps going past what any single thought needs,
// which is exactly the shape this rule exists to price,
// and it ends here on the fourth line.
pub fn subtract(a: i32, b: i32) -> i32 {
    a - b
}

/// A doc block long enough to trip max_doc_consecutive.
/// Every line here is prose on an adjacent line.
/// None of it is fenced as an example.
/// So all five lines count as one doc run,
/// ending on this fifth line.
pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

pub fn divide(a: i32, b: i32) -> i32 {
    a / b
}
