use std::io::Write;

pub fn init_logger(prefix: &'static str) {
    env_logger::Builder::from_default_env()
        .format(move |buf, record| {
            writeln!(
                buf,
                "[{} {} {}] {}",
                buf.timestamp(),
                record.level(),
                prefix,
                record.args()
            )
        })
        .init();
}
