use super::{run_stage, ClientMode};
use crate::scenarios::c2c_broker_oracle_report::{
    print_calibration_line, snapshot_delta, BROKER_CALIBRATION_SAMPLES,
};

pub fn run() -> usize {
    let (summaries, broker_tid, before, after) = run_stage(
        1,
        50,
        ClientMode::EchoCalibrate,
        BROKER_CALIBRATION_SAMPLES,
        0,
        false,
    );
    print_calibration_line(summaries[0], snapshot_delta(before, after));
    broker_tid
}
