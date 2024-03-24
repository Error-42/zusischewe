use std::{error::Error, fmt::Display, str::FromStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Datetime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Display for Datetime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        )
    }
}

impl FromStr for Datetime {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let [date, time]: [&str; 2] = s
            .split(' ')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| "expected two parts separated by space")?;

        let date: [&str; 3] = date
            .split('-')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| "date note consisting of three parts")?;

        let year = date[0].parse()?;
        let month = date[1].parse()?;
        let day = date[2].parse()?;

        let time: [&str; 3] = time
            .split(':')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| "time note consisting of three parts")?;

        let hour = time[0].parse()?;
        let minute = time[1].parse()?;
        let second = time[2].parse()?;

        Ok(
            Datetime {
                year,
                month,
                day,
                hour,
                minute,
                second,
            }
        )
    }
}

impl Datetime {
    /// It doesn't handle date transitions or leap seconds. In these cases it produces some time withing the same date.
    pub fn inc_seconds(&mut self, inc: u32) {
        let total = self.hour as u32 * 3600 + self.minute as u32 * 60 + self.second as u32 + inc;

        self.hour = (total / 3600).min(23) as u8;
        let total = total - self.hour as u32 * 3600;

        self.minute = (total / 60).min(59) as u8;
        let total = total - self.minute as u32 * 60;

        self.second = total.min(59) as u8;
    }
}
