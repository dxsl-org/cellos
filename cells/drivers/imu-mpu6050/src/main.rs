#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use api::declare_manifest;
use driver_i2c_bcm::BcmBscI2c;
use driver_imu_mpu6050::{Mpu6050, DEFAULT_I2C_ADDR};
use ostd::io::println;

declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false,
    i2c = true,
    spi = false
);

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[mpu6050] MPU6050 6-DOF IMU driver cell starting...");

    let i2c = match BcmBscI2c::open() {
        Ok(i2c) => {
            println("[mpu6050] BCM BSC1 I2C controller acquired");
            i2c
        }
        Err(_) => {
            println("[mpu6050] failed to acquire BCM BSC1 I2C controller");
            return;
        }
    };

    let mut imu = Mpu6050::new(i2c, DEFAULT_I2C_ADDR);
    match imu.init() {
        Ok(id) if id == 0x68 => {
            println("[mpu6050] WHO_AM_I: 0x68 (MPU6050 verified)");
        }
        Ok(id) => {
            println("[mpu6050] unexpected WHO_AM_I id");
            let _ = id;
            return;
        }
        Err(e) => {
            println("[mpu6050] I2C communication error during init");
            let _ = e;
            return;
        }
    }

    if let Ok(data) = imu.read_sensor() {
        println("[mpu6050] IMU read sample acquired:");
        let _ = data;
    }
}
