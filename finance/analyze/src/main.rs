use simple_logger::SimpleLogger;

fn main() {
    SimpleLogger::new()
        .with_timestamp_format(time::macros::format_description!(
            "[day] [hour]:[minute]:[second]"
        ))
        .with_level(log::LevelFilter::Debug)
        .init()
        .unwrap();

    log::debug!("TESTING!");
}
