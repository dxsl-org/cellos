use alloc::string::String;
use alloc::vec::Vec;

use super::{AwkError, BinaryOp, Builtin, Expr, Filter, Program, Separator, UnaryOp};
use crate::matcher::{CompiledPattern, PatternKind};
use crate::records::{RecordReader, MAX_RECORD_BYTES};

pub(crate) fn run(
    program: &Program,
    separator: Separator<'_>,
    input: &str,
) -> Result<Vec<String>, AwkError> {
    let regex = match &program.filter {
        Some(Filter::Regex(pattern)) => Some(
            CompiledPattern::compile(PatternKind::ERELite, pattern, false)
                .map_err(|_| AwkError::Runtime("invalid regex filter"))?,
        ),
        _ => None,
    };
    let records = RecordReader::new(input)
        .map_err(AwkError::Record)?
        .collect()
        .map_err(AwkError::Record)?;
    let mut output = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let fields = split_fields(record, separator);
        let env = Env {
            line: record,
            fields: &fields,
            nr: index + 1,
        };
        if !selected(program.filter.as_ref(), regex.as_ref(), &env)? {
            continue;
        }
        let line = render(program, &env)?;
        if line.len() > MAX_RECORD_BYTES {
            return Err(AwkError::Runtime("output record exceeds configured limit"));
        }
        output.push(line);
    }
    Ok(output)
}

fn split_fields<'a>(line: &'a str, separator: Separator<'_>) -> Vec<&'a str> {
    match separator {
        Separator::Whitespace => line.split_whitespace().collect(),
        Separator::Literal(sep) => line.split(sep).collect(),
    }
}

fn selected(
    filter: Option<&Filter>,
    regex: Option<&CompiledPattern>,
    env: &Env<'_>,
) -> Result<bool, AwkError> {
    match filter {
        None => Ok(true),
        Some(Filter::Regex(_)) => Ok(regex
            .map(|compiled| compiled.is_match(env.line))
            .unwrap_or(false)),
        Some(Filter::Expr(expr)) => Ok(eval(expr, env)?.truthy()),
    }
}

fn render(program: &Program, env: &Env<'_>) -> Result<String, AwkError> {
    if program.print.is_empty() {
        return Ok(String::from(env.line));
    }
    let mut out = String::new();
    for (index, expr) in program.print.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(&eval(expr, env)?.text());
    }
    Ok(out)
}

struct Env<'a> {
    line: &'a str,
    fields: &'a [&'a str],
    nr: usize,
}

#[derive(Clone)]
enum Value {
    Num(i64),
    Text(String),
    Bool(bool),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Self::Num(value) => *value != 0,
            Self::Text(value) => !value.is_empty(),
            Self::Bool(value) => *value,
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Num(value) => alloc::format!("{value}"),
            Self::Text(value) => value.clone(),
            Self::Bool(value) => alloc::format!("{}", if *value { 1 } else { 0 }),
        }
    }

    fn num(&self) -> Result<i64, AwkError> {
        match self {
            Self::Num(value) => Ok(*value),
            Self::Bool(value) => Ok(if *value { 1 } else { 0 }),
            Self::Text(value) => value
                .parse::<i64>()
                .map_err(|_| AwkError::Runtime("numeric operation requires integers")),
        }
    }
}

fn eval(expr: &Expr, env: &Env<'_>) -> Result<Value, AwkError> {
    match expr {
        Expr::Number(value) => Ok(Value::Num(*value)),
        Expr::Text(value) => Ok(Value::Text(value.clone())),
        Expr::Field(0) => Ok(Value::Text(String::from(env.line))),
        Expr::Field(field) => Ok(Value::Text(String::from(
            env.fields.get(*field as usize - 1).copied().unwrap_or(""),
        ))),
        Expr::Builtin(Builtin::Nr) => Ok(Value::Num(env.nr as i64)),
        Expr::Builtin(Builtin::Nf) => Ok(Value::Num(env.fields.len() as i64)),
        Expr::Unary { op, expr } => {
            let value = eval(expr, env)?;
            match op {
                UnaryOp::Neg => Ok(Value::Num(-value.num()?)),
                UnaryOp::Not => Ok(Value::Bool(!value.truthy())),
            }
        }
        Expr::Binary { op, lhs, rhs } => eval_binary(*op, &eval(lhs, env)?, rhs, env),
    }
}

fn eval_binary(op: BinaryOp, lhs: &Value, rhs: &Expr, env: &Env<'_>) -> Result<Value, AwkError> {
    match op {
        BinaryOp::And => Ok(Value::Bool(lhs.truthy() && eval(rhs, env)?.truthy())),
        BinaryOp::Or => Ok(Value::Bool(lhs.truthy() || eval(rhs, env)?.truthy())),
        BinaryOp::Add => Ok(Value::Num(lhs.num()? + eval(rhs, env)?.num()?)),
        BinaryOp::Sub => Ok(Value::Num(lhs.num()? - eval(rhs, env)?.num()?)),
        BinaryOp::Mul => Ok(Value::Num(lhs.num()? * eval(rhs, env)?.num()?)),
        BinaryOp::Div => {
            let rhs = eval(rhs, env)?.num()?;
            if rhs == 0 {
                return Err(AwkError::Runtime("division by zero"));
            }
            Ok(Value::Num(lhs.num()? / rhs))
        }
        BinaryOp::Rem => {
            let rhs = eval(rhs, env)?.num()?;
            if rhs == 0 {
                return Err(AwkError::Runtime("division by zero"));
            }
            Ok(Value::Num(lhs.num()? % rhs))
        }
        op => {
            let rhs = eval(rhs, env)?;
            Ok(Value::Bool(compare(op, lhs, &rhs)?))
        }
    }
}

fn compare(op: BinaryOp, lhs: &Value, rhs: &Value) -> Result<bool, AwkError> {
    let numeric = lhs.num().ok().zip(rhs.num().ok());
    if let Some((left, right)) = numeric {
        return Ok(match op {
            BinaryOp::Eq => left == right,
            BinaryOp::Ne => left != right,
            BinaryOp::Lt => left < right,
            BinaryOp::Le => left <= right,
            BinaryOp::Gt => left > right,
            BinaryOp::Ge => left >= right,
            _ => return Err(AwkError::Runtime("invalid comparison")),
        });
    }
    let left = lhs.text();
    let right = rhs.text();
    Ok(match op {
        BinaryOp::Eq => left == right,
        BinaryOp::Ne => left != right,
        BinaryOp::Lt => left < right,
        BinaryOp::Le => left <= right,
        BinaryOp::Gt => left > right,
        BinaryOp::Ge => left >= right,
        _ => return Err(AwkError::Runtime("invalid comparison")),
    })
}
