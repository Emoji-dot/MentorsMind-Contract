//! Shared input validation middleware for all MentorsMind contracts.
//!
//! Provides a [`Validator`] builder pattern for consistent, reusable input
//! validation across contract entry points. Each validation rule returns
//! `Self` for chaining, and `validate()` produces a `Result<(), ValidationError>`.
//!
//! # Example
//! ```ignore
//! Validator::new(&env)
//!     .require_positive(amount, "amount")
//!     .require_future_timestamp(deadline, "deadline")
//!     .require_range(duration_mins, 1, 480, "duration_mins")
//!     .validate()?;
//! ```

use soroban_sdk::{contracttype, Env, Symbol, Vec};

/// Error returned when a validation rule fails.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationError {
    /// The name of the field that failed validation.
    pub field: Symbol,
    /// The constraint that was violated (e.g. "positive", "future", "range").
    pub constraint: Symbol,
    /// The value that was provided (for numeric fields; 0 for non-numeric).
    pub value_provided: i128,
}

/// Builder for validating contract inputs.
///
/// Collects validation errors without short-circuiting, so all failures
/// can be reported at once (useful for batch operations).
pub struct Validator<'a> {
    env: &'a Env,
    errors: Vec<ValidationError>,
}

impl<'a> Validator<'a> {
    pub fn new(env: &'a Env) -> Self {
        Self {
            env,
            errors: Vec::new(env),
        }
    }

    /// Assert that `value` is strictly positive (> 0).
    pub fn require_positive(mut self, value: i128, field: &str) -> Self {
        if value <= 0 {
            self.errors.push_back(ValidationError {
                field: Symbol::new(self.env, field),
                constraint: Symbol::new(self.env, "positive"),
                value_provided: value,
            });
        }
        self
    }

    /// Assert that `value` is non-negative (>= 0).
    pub fn require_non_negative(mut self, value: i128, field: &str) -> Self {
        if value < 0 {
            self.errors.push_back(ValidationError {
                field: Symbol::new(self.env, field),
                constraint: Symbol::new(self.env, "non_negative"),
                value_provided: value,
            });
        }
        self
    }

    /// Assert that `timestamp` is in the future relative to the ledger.
    pub fn require_future_timestamp(mut self, timestamp: u64, field: &str) -> Self {
        let now = self.env.ledger().timestamp();
        if timestamp <= now {
            self.errors.push_back(ValidationError {
                field: Symbol::new(self.env, field),
                constraint: Symbol::new(self.env, "future"),
                value_provided: timestamp as i128,
            });
        }
        self
    }

    /// Assert that `value` is within `[min, max]` (inclusive).
    pub fn require_range(mut self, value: i128, min: i128, max: i128, field: &str) -> Self {
        if value < min || value > max {
            self.errors.push_back(ValidationError {
                field: Symbol::new(self.env, field),
                constraint: Symbol::new(self.env, "range"),
                value_provided: value,
            });
        }
        self
    }

    /// Assert that `value` is non-zero (for addresses, hashes, etc.).
    pub fn require_nonzero(mut self, value: i128, field: &str) -> Self {
        if value == 0 {
            self.errors.push_back(ValidationError {
                field: Symbol::new(self.env, field),
                constraint: Symbol::new(self.env, "nonzero"),
                value_provided: 0,
            });
        }
        self
    }

    /// Consume the builder and return `Ok(())` if all rules passed, or
    /// `Err(ValidationError)` with the first failure.
    pub fn validate(self) -> Result<(), ValidationError> {
        match self.errors.iter().next() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Consume the builder and return all collected errors (empty = all passed).
    pub fn validate_all(self) -> Vec<ValidationError> {
        self.errors
    }
}

/// Convenience: combine `require_auth()` with `Validator::validate()`.
///
/// Calls `caller.require_auth()` first, then runs the validator. If auth
/// fails, Soroban panics before validation runs (matching existing contract
/// behavior).
pub fn require_auth_and_validate(
    caller: &soroban_sdk::Address,
    validator: Validator<'_>,
) -> Result<(), ValidationError> {
    caller.require_auth();
    validator.validate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_positive_passes_for_positive_value() {
        let env = Env::default();
        let result = Validator::new(&env).require_positive(100, "amount").validate();
        assert!(result.is_ok());
    }

    #[test]
    fn require_positive_fails_for_zero() {
        let env = Env::default();
        let err = Validator::new(&env)
            .require_positive(0, "amount")
            .validate()
            .unwrap_err();
        assert_eq!(err.field, Symbol::new(&env, "amount"));
        assert_eq!(err.constraint, Symbol::new(&env, "positive"));
    }

    #[test]
    fn require_positive_fails_for_negative() {
        let env = Env::default();
        let err = Validator::new(&env)
            .require_positive(-5, "amount")
            .validate()
            .unwrap_err();
        assert_eq!(err.value_provided, -5);
    }

    #[test]
    fn require_range_passes_within_bounds() {
        let env = Env::default();
        let result = Validator::new(&env)
            .require_range(200, 1, 480, "duration")
            .validate();
        assert!(result.is_ok());
    }

    #[test]
    fn require_range_fails_below_min() {
        let env = Env::default();
        let err = Validator::new(&env)
            .require_range(0, 1, 480, "duration")
            .validate()
            .unwrap_err();
        assert_eq!(err.constraint, Symbol::new(&env, "range"));
    }

    #[test]
    fn require_range_fails_above_max() {
        let env = Env::default();
        let err = Validator::new(&env)
            .require_range(500, 1, 480, "duration")
            .validate()
            .unwrap_err();
        assert_eq!(err.value_provided, 500);
    }

    #[test]
    fn chained_validations_all_must_pass() {
        let env = Env::default();
        let result = Validator::new(&env)
            .require_positive(100, "amount")
            .require_range(60, 1, 480, "duration")
            .validate();
        assert!(result.is_ok());
    }

    #[test]
    fn validate_all_returns_multiple_errors() {
        let env = Env::default();
        let errors = Validator::new(&env)
            .require_positive(0, "amount")
            .require_range(0, 1, 480, "duration")
            .validate_all();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn require_non_negative_passes_for_zero() {
        let env = Env::default();
        let result = Validator::new(&env)
            .require_non_negative(0, "fee")
            .validate();
        assert!(result.is_ok());
    }

    #[test]
    fn require_non_negative_fails_for_negative() {
        let env = Env::default();
        let err = Validator::new(&env)
            .require_non_negative(-1, "fee")
            .validate()
            .unwrap_err();
        assert_eq!(err.constraint, Symbol::new(&env, "non_negative"));
    }
}
