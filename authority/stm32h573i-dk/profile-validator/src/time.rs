use crate::{der::Element, Error};

pub(crate) fn parse(element: Element<'_>) -> Result<i64, Error> {
    let (year, rest) = match element.tag {
        0x17 if element.value.len() == 13 => {
            let year = decimal(&element.value[..2])? as i32;
            (
                if year >= 50 { 1900 + year } else { 2000 + year },
                &element.value[2..],
            )
        }
        0x18 if element.value.len() == 15 => {
            let year = decimal(&element.value[..4])? as i32;
            if year < 2050 {
                return Err(Error::InvalidValidity);
            }
            (year, &element.value[4..])
        }
        _ => return Err(Error::InvalidValidity),
    };
    if rest[10] != b'Z' {
        return Err(Error::InvalidValidity);
    }
    let month = decimal(&rest[0..2])?;
    let day = decimal(&rest[2..4])?;
    let hour = decimal(&rest[4..6])?;
    let minute = decimal(&rest[6..8])?;
    let second = decimal(&rest[8..10])?;
    if !(1..=12).contains(&month)
        || day == 0
        || day > month_days(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(Error::InvalidValidity);
    }
    let days = days_from_civil(year, month, day);
    days.checked_mul(86_400)
        .and_then(|v| v.checked_add((hour * 3600 + minute * 60 + second) as i64))
        .ok_or(Error::InvalidValidity)
}

fn decimal(bytes: &[u8]) -> Result<u32, Error> {
    let mut value = 0u32;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(Error::InvalidValidity);
        }
        value = value * 10 + (byte - b'0') as u32;
    }
    Ok(value)
}

fn month_days(year: i32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let shifted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted_month + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
}
