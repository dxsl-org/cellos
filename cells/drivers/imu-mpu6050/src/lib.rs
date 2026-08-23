#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use hal_i2c::ViI2c;

pub const DEFAULT_I2C_ADDR: u8 = 0x68;
pub const ALT_I2C_ADDR: u8 = 0x69;

const REG_SMPLRT_DIV: u8 = 0x19;
const REG_CONFIG: u8 = 0x1A;
const REG_GYRO_CONFIG: u8 = 0x1B;
const REG_ACCEL_CONFIG: u8 = 0x1C;
const REG_ACCEL_XOUT_H: u8 = 0x3B;
const REG_PWR_MGMT_1: u8 = 0x6B;
const REG_WHO_AM_I: u8 = 0x75;

const WHO_AM_I_EXPECTED: u8 = 0x68;

#[derive(Clone, Copy, Debug, Default)]
pub struct ImuData {
    pub accel_x: i16,
    pub accel_y: i16,
    pub accel_z: i16,
    pub temp_raw: i16,
    pub gyro_x: i16,
    pub gyro_y: i16,
    pub gyro_z: i16,
}

pub struct Mpu6050<I2C> {
    i2c: I2C,
    addr: u8,
}

impl<I2C: ViI2c> Mpu6050<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self { i2c, addr }
    }

    pub fn init(&mut self) -> Result<u8, I2C::Error> {
        let mut who_am_i = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[REG_WHO_AM_I], &mut who_am_i)?;
        let id = who_am_i[0];
        if id != WHO_AM_I_EXPECTED {
            return Ok(id);
        }

        // Wake device from sleep mode (clear SLEEP bit 6 in PWR_MGMT_1)
        self.i2c.write(self.addr, &[REG_PWR_MGMT_1, 0x00])?;
        // Set sample rate divider (1 kHz / (1 + 7) = 125 Hz)
        self.i2c.write(self.addr, &[REG_SMPLRT_DIV, 0x07])?;
        // DLPF cfg = 3 (44 Hz Accel, 42 Hz Gyro bandwidth)
        self.i2c.write(self.addr, &[REG_CONFIG, 0x03])?;
        // Gyro range = ±500 deg/s (FS_SEL = 1)
        self.i2c.write(self.addr, &[REG_GYRO_CONFIG, 0x08])?;
        // Accel range = ±2g (AFS_SEL = 0)
        self.i2c.write(self.addr, &[REG_ACCEL_CONFIG, 0x00])?;

        Ok(id)
    }

    pub fn read_sensor(&mut self) -> Result<ImuData, I2C::Error> {
        let mut buf = [0u8; 14];
        self.i2c
            .write_read(self.addr, &[REG_ACCEL_XOUT_H], &mut buf)?;

        let accel_x = i16::from_be_bytes([buf[0], buf[1]]);
        let accel_y = i16::from_be_bytes([buf[2], buf[3]]);
        let accel_z = i16::from_be_bytes([buf[4], buf[5]]);
        let temp_raw = i16::from_be_bytes([buf[6], buf[7]]);
        let gyro_x = i16::from_be_bytes([buf[8], buf[9]]);
        let gyro_y = i16::from_be_bytes([buf[10], buf[11]]);
        let gyro_z = i16::from_be_bytes([buf[12], buf[13]]);

        Ok(ImuData {
            accel_x,
            accel_y,
            accel_z,
            temp_raw,
            gyro_x,
            gyro_y,
            gyro_z,
        })
    }
}
