use crate::tool::Tool;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::error::Error;
use std::fmt;

#[cfg(feature = "web")]
use tracing::debug;

#[derive(Debug)]
pub enum CalculatorError {
    InvalidToken(char),
    MismatchedParentheses,
    DivisionByZero,
    ModuloByZero,
    UnexpectedEndOfInput,
    FactorialOfNegative(f64),
    FactorialOfNonInteger(f64),
    FactorialOverflow(u64),
}

impl fmt::Display for CalculatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalculatorError::InvalidToken(c) => write!(f, "Invalid token: '{}'", c),
            CalculatorError::MismatchedParentheses => write!(f, "Mismatched parentheses"),
            CalculatorError::DivisionByZero => write!(f, "Division by zero"),
            CalculatorError::ModuloByZero => write!(f, "Modulo by zero"),
            CalculatorError::UnexpectedEndOfInput => write!(f, "Unexpected end of input"),
            CalculatorError::FactorialOfNegative(n) => {
                write!(f, "Factorial of negative number: {}", n)
            }
            CalculatorError::FactorialOfNonInteger(n) => {
                write!(f, "Factorial of non-integer: {}", n)
            }
            CalculatorError::FactorialOverflow(n) => write!(
                f,
                "Factorial overflow: {}! exceeds u64 (maximum supported operand is 20)",
                n
            ),
        }
    }
}

impl Error for CalculatorError {}

#[derive(Debug, PartialEq, Clone)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Caret,
    Factorial,
    LeftParen,
    RightParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, CalculatorError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' => continue,
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Multiply),
            '/' => tokens.push(Token::Divide),
            '%' => tokens.push(Token::Modulo),
            '^' => tokens.push(Token::Caret),
            '!' => tokens.push(Token::Factorial),
            '(' => tokens.push(Token::LeftParen),
            ')' => tokens.push(Token::RightParen),
            _ => {
                if c.is_ascii_digit() || c == '.' {
                    let mut num_str = String::new();
                    num_str.push(c);
                    while let Some(&next_c) = chars.peek() {
                        if next_c.is_ascii_digit() || next_c == '.' {
                            num_str.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    match num_str.parse::<f64>() {
                        Ok(num) => tokens.push(Token::Number(num)),
                        Err(_) => return Err(CalculatorError::InvalidToken(c)),
                    }
                } else {
                    return Err(CalculatorError::InvalidToken(c));
                }
            }
        }
    }
    Ok(tokens)
}

#[derive(Debug)]
enum Expr {
    Number(f64),
    Add(Box<Expr>, Box<Expr>),
    Subtract(Box<Expr>, Box<Expr>),
    Multiply(Box<Expr>, Box<Expr>),
    Divide(Box<Expr>, Box<Expr>),
    Modulo(Box<Expr>, Box<Expr>),
    Exponent(Box<Expr>, Box<Expr>),
    Factorial(Box<Expr>),
    Negate(Box<Expr>),
}

fn parse(tokens: &[Token]) -> Result<Expr, CalculatorError> {
    let mut index = 0;
    let expr = parse_expression(tokens, &mut index)?;
    if index != tokens.len() {
        return Err(CalculatorError::UnexpectedEndOfInput);
    }
    Ok(expr)
}

fn parse_expression(tokens: &[Token], index: &mut usize) -> Result<Expr, CalculatorError> {
    let mut left = parse_term(tokens, index)?;
    while *index < tokens.len() {
        match &tokens[*index] {
            Token::Plus => {
                *index += 1;
                let right = parse_term(tokens, index)?;
                left = Expr::Add(Box::new(left), Box::new(right));
            }
            Token::Minus => {
                *index += 1;
                let right = parse_term(tokens, index)?;
                left = Expr::Subtract(Box::new(left), Box::new(right));
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_term(tokens: &[Token], index: &mut usize) -> Result<Expr, CalculatorError> {
    let mut left = parse_exponent(tokens, index)?;
    while *index < tokens.len() {
        match &tokens[*index] {
            Token::Multiply => {
                *index += 1;
                let right = parse_exponent(tokens, index)?;
                left = Expr::Multiply(Box::new(left), Box::new(right));
            }
            Token::Divide => {
                *index += 1;
                let right = parse_exponent(tokens, index)?;
                left = Expr::Divide(Box::new(left), Box::new(right));
            }
            Token::Modulo => {
                *index += 1;
                let right = parse_exponent(tokens, index)?;
                left = Expr::Modulo(Box::new(left), Box::new(right));
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_exponent(tokens: &[Token], index: &mut usize) -> Result<Expr, CalculatorError> {
    let mut left = parse_factor(tokens, index)?;
    while *index < tokens.len() {
        match &tokens[*index] {
            Token::Caret => {
                // Right-associative: recurse on parse_exponent so 2^3^2 = 2^(3^2) = 512.
                *index += 1;
                let right = parse_exponent(tokens, index)?;
                left = Expr::Exponent(Box::new(left), Box::new(right));
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_factor(tokens: &[Token], index: &mut usize) -> Result<Expr, CalculatorError> {
    let mut expr = parse_postfix(tokens, index)?;
    while *index < tokens.len() && matches!(tokens[*index], Token::Factorial) {
        *index += 1;
        expr = Expr::Factorial(Box::new(expr));
    }
    Ok(expr)
}

fn parse_postfix(tokens: &[Token], index: &mut usize) -> Result<Expr, CalculatorError> {
    if *index >= tokens.len() {
        return Err(CalculatorError::UnexpectedEndOfInput);
    }
    match &tokens[*index] {
        Token::Number(n) => {
            *index += 1;
            Ok(Expr::Number(*n))
        }
        Token::LeftParen => {
            *index += 1;
            let expr = parse_expression(tokens, index)?;
            if *index < tokens.len() && matches!(tokens[*index], Token::RightParen) {
                *index += 1;
                Ok(expr)
            } else {
                Err(CalculatorError::MismatchedParentheses)
            }
        }
        Token::Minus => {
            *index += 1;
            let expr = parse_postfix(tokens, index)?;
            Ok(Expr::Negate(Box::new(expr)))
        }
        _ => Err(CalculatorError::UnexpectedEndOfInput),
    }
}

/// Max operand whose factorial fits in u64: 20! = 2_432_902_008_176_640_000,
/// 21! overflows.
const MAX_FACTORIAL_OPERAND: u64 = 20;

fn factorial(n: u64) -> u64 {
    if n == 0 { 1 } else { n * factorial(n - 1) }
}

fn evaluate(expr: &Expr) -> Result<f64, CalculatorError> {
    match expr {
        Expr::Number(n) => Ok(*n),
        Expr::Add(l, r) => Ok(evaluate(l)? + evaluate(r)?),
        Expr::Subtract(l, r) => Ok(evaluate(l)? - evaluate(r)?),
        Expr::Multiply(l, r) => Ok(evaluate(l)? * evaluate(r)?),
        Expr::Divide(l, r) => {
            let (lv, rv) = (evaluate(l)?, evaluate(r)?);
            if rv == 0.0 {
                return Err(CalculatorError::DivisionByZero);
            }
            Ok(lv / rv)
        }
        Expr::Modulo(l, r) => {
            let (lv, rv) = (evaluate(l)?, evaluate(r)?);
            if rv == 0.0 {
                return Err(CalculatorError::ModuloByZero);
            }
            Ok(lv % rv)
        }
        Expr::Exponent(l, r) => Ok(evaluate(l)?.powf(evaluate(r)?)),
        Expr::Factorial(e) => {
            let val = evaluate(e)?;
            if val < 0.0 {
                return Err(CalculatorError::FactorialOfNegative(val));
            }
            if val.fract() != 0.0 {
                return Err(CalculatorError::FactorialOfNonInteger(val));
            }
            let n = val as u64;
            if n > MAX_FACTORIAL_OPERAND {
                return Err(CalculatorError::FactorialOverflow(n));
            }
            Ok(factorial(n) as f64)
        }
        Expr::Negate(e) => Ok(-evaluate(e)?),
    }
}

/// Evaluate an arithmetic expression: numbers (f64), `+ - * / % ^ ! ( )`,
/// unary minus, parens. `^` is right-associative; `!` is postfix factorial.
pub fn calculate(input: &str) -> Result<f64, CalculatorError> {
    let tokens = tokenize(input)?;
    let expr = parse(&tokens)?;
    evaluate(&expr)
}

pub struct Calculator;

#[derive(Debug, Deserialize, Serialize)]
struct CalculatorInput {
    expression: String,
}

#[async_trait]
impl Tool for Calculator {
    fn name(&self) -> String {
        "calculator".to_string()
    }

    fn description(&self) -> String {
        "Evaluate an arithmetic expression and return the result. Syntax: \
         numbers (integers or decimals), operators + - * / % (add, subtract, \
         multiply, divide, modulo), ^ for exponentiation (right-associative, \
         e.g. 2^3^2 = 512), ! for postfix factorial (integers 0-20 only, e.g. \
         5! = 120), parentheses for grouping, and unary minus (e.g. -3 + 5). \
         Examples: \"2 ^ 7.2\", \"3 + 4 * 2 / (1-5)^2\", \"5!\"."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "The arithmetic expression to evaluate, e.g. \
                                    \"3 + 4 * 2 / (1-5)^2\". Supports + - * / %, \
                                    right-associative ^, postfix ! (factorial, 0-20), \
                                    parentheses, and unary minus."
                }
            },
            "required": ["expression"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let input: CalculatorInput = serde_json::from_value(input).map_err(|e| {
            anyhow!(
                "Invalid calculator input: expected {{\"expression\": \"...\"}} \
                 (arithmetic expression string, e.g. \"2^7.2\"); got: {}",
                e
            )
        })?;

        #[cfg(feature = "web")]
        debug!("Calculator: expression={}", input.expression);

        let result = calculate(&input.expression).map_err(|e| anyhow!("calculator: {}", e))?;

        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- calculate(): user's unit tests (ported) ----

    #[test]
    fn precedence_mul_over_add() {
        assert_eq!(calculate("3 + 4 * 2").unwrap(), 11.0);
    }

    #[test]
    fn parens_and_division() {
        assert_eq!(calculate("3 + 4 * 2 / (1-5)^2").unwrap(), 3.5);
    }

    #[test]
    fn exponent_right_associative() {
        assert_eq!(calculate("2^3^2").unwrap(), 512.0);
    }

    #[test]
    fn exponent_float_operand() {
        let got = calculate("2 ^ 7.2").unwrap();
        assert!((got - 147.033).abs() < 0.01, "got {got}");
    }

    #[test]
    fn factorial() {
        assert_eq!(calculate("5!").unwrap(), 120.0);
        assert_eq!(calculate("0!").unwrap(), 1.0);
    }

    #[test]
    fn factorial_of_negative_errors() {
        assert!(matches!(
            calculate("-5!"),
            Err(CalculatorError::FactorialOfNegative(n)) if n == -5.0
        ));
    }

    #[test]
    fn factorial_of_non_integer_errors() {
        assert!(matches!(
            calculate("5.5!"),
            Err(CalculatorError::FactorialOfNonInteger(n)) if n == 5.5
        ));
    }

    #[test]
    fn modulo() {
        assert_eq!(calculate("7 % 3").unwrap(), 1.0);
    }

    #[test]
    fn modulo_by_zero_errors() {
        assert!(matches!(
            calculate("5 % 0"),
            Err(CalculatorError::ModuloByZero)
        ));
    }

    #[test]
    fn division_by_zero_errors() {
        assert!(matches!(
            calculate("1 / 0"),
            Err(CalculatorError::DivisionByZero)
        ));
    }

    #[test]
    fn unary_minus() {
        assert_eq!(calculate("-3 + 5").unwrap(), 2.0);
        assert_eq!(calculate("2 * -3").unwrap(), -6.0);
    }

    #[test]
    fn floats() {
        assert_eq!(calculate("1.5 + 2.25").unwrap(), 3.75);
    }

    #[test]
    fn mismatched_parentheses_errors() {
        assert!(matches!(
            calculate("(2 + 3"),
            Err(CalculatorError::MismatchedParentheses)
        ));
    }

    #[test]
    fn complex_expression() {
        // (2+3)*(4-1) = 15, 15! = 1_307_674_368_000
        assert_eq!(calculate("((2+3)*(4-1))!").unwrap(), 1_307_674_368_000.0);
    }

    #[test]
    fn invalid_token_errors() {
        assert!(matches!(
            calculate("2 & 3"),
            Err(CalculatorError::InvalidToken('&'))
        ));
    }

    #[test]
    fn unexpected_end_of_input_errors() {
        assert!(matches!(
            calculate("2 +"),
            Err(CalculatorError::UnexpectedEndOfInput)
        ));
    }

    #[test]
    fn empty_input_errors() {
        assert!(matches!(
            calculate(""),
            Err(CalculatorError::UnexpectedEndOfInput)
        ));
    }

    // ---- item38 TDD additions ----

    #[test]
    fn factorial_overflow_guard() {
        assert!(matches!(
            calculate("21!"),
            Err(CalculatorError::FactorialOverflow(21))
        ));
        // Boundary: 20! fits in u64.
        assert_eq!(calculate("20!").unwrap(), 2_432_902_008_176_640_000.0);
    }

    #[test]
    fn factorial_overflow_display() {
        let err = CalculatorError::FactorialOverflow(21);
        let msg = format!("{}", err);
        assert!(msg.contains("overflow"), "display: {msg}");
        assert!(msg.contains("21"), "display: {msg}");
    }

    // ---- tool-level ----

    #[tokio::test]
    async fn execute_new_shape_expression() {
        let out = Calculator
            .execute(json!({"expression": "2^7.2"}))
            .await
            .expect("expression input should work");
        let got: f64 = out.parse().expect("result should be an f64 string");
        assert!((got - 147.033).abs() < 0.01, "2^7.2 -> got {out}");
    }

    #[tokio::test]
    async fn execute_old_shape_errors_clearly() {
        let out = Calculator
            .execute(json!({"operation": "add", "a": 1.0, "b": 2.0}))
            .await;
        assert!(out.is_err(), "old operation/a/b shape must be rejected");
        let msg = format!("{}", out.unwrap_err());
        assert!(
            msg.to_lowercase().contains("expression"),
            "error should mention the expected shape: {msg}"
        );
    }

    #[tokio::test]
    async fn execute_missing_input_errors() {
        assert!(Calculator.execute(json!({})).await.is_err());
    }

    #[tokio::test]
    async fn execute_schema_requires_expression() {
        let schema = Calculator.input_schema();
        assert_eq!(schema["required"], json!(["expression"]));
        assert!(schema["properties"]["expression"].is_object());
        assert!(schema["properties"]["operation"].is_null());
    }
}
