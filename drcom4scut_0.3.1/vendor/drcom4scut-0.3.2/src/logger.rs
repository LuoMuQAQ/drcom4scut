use crate::Settings;

#[allow(unused_variables)]
pub fn init(settings: &Settings) {
    #[cfg(not(feature = "log4rs"))]
    simple_logger::init();
    #[cfg(feature = "log4rs")]
    init_log4rs(settings);
}

#[cfg(not(feature = "log4rs"))]
mod simple_logger {
    use log::{LevelFilter, Metadata, Record};

    static LOGGER: SimpleLogger = SimpleLogger;

    pub fn init() {
        log::set_logger(&LOGGER)
            .map(|()| log::set_max_level(LevelFilter::Trace))
            .expect("Failed to init logger!");
    }

    struct SimpleLogger;

    impl log::Log for SimpleLogger {
        #[inline]
        fn enabled(&self, _: &Metadata) -> bool {
            true
        }
        fn log(&self, record: &Record) {
            let dt = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let thread_name = std::thread::current()
                .clone()
                .name()
                .unwrap_or("unnamed")
                .to_string();
            println!(
                "[{}][{}][{}] {}",
                dt,
                record.level(),
                thread_name,
                record.args()
            );
        }
        fn flush(&self) {}
    }
}

#[cfg(feature = "log4rs")]
fn init_log4rs(settings: &Settings) {
    if log::LevelFilter::Off == settings.log.level_filter {
        return;
    }
    use crate::settings::DEBUG;
    use log4rs::{
        append::{
            console::ConsoleAppender,
            rolling_file::{
                RollingFileAppender,
                policy::compound::{
                    CompoundPolicy, roll::fixed_window::FixedWindowRoller,
                    trigger::size::SizeTrigger,
                },
            },
        },
        config::{Appender, Config, Root},
        encode::pattern::PatternEncoder,
    };
    let stdout = if *DEBUG || settings.log.enable_console {
        Some(
            ConsoleAppender::builder()
                .encoder(Box::new(PatternEncoder::new(
                    "{h([{d(%Y-%m-%d %H:%M:%S)}][{l}][{T}] {m}{n})}",
                )))
                .build(),
        )
    } else {
        None
    };
    let logfile = if settings.log.enable_file {
        let directory = &settings.log.file_directory;
        if let Err(e) = std::fs::create_dir_all(directory) {
            eprintln!("Can't create log directory '{directory}': {e}. File log disabled.");
            None
        } else {
            let roller = FixedWindowRoller::builder()
                .base(1)
                .build(&(directory.clone() + "/log-{}.gz"), 10);
            let appender = roller.ok().and_then(|roller| {
                RollingFileAppender::builder()
                    .encoder(Box::new(PatternEncoder::new(
                        "[{d(%Y-%m-%d %H:%M:%S)}][{l}][{T}][{M}:{L}] {m}{n}",
                    )))
                    .build(
                        directory.clone() + "/latest.log",
                        Box::new(CompoundPolicy::new(
                            Box::new(SizeTrigger::new(1 << 20)),
                            Box::new(roller),
                        )),
                    )
                    .ok()
            });
            if appender.is_none() {
                eprintln!("Can't build RollingFileAppender in '{directory}'. File log disabled.");
            }
            appender
        }
    } else {
        None
    };

    if stdout.is_none() && logfile.is_none() {
        return;
    }

    let mut config = Config::builder();
    let mut root = Root::builder();
    if let Some(stdout) = stdout {
        config = config.appender(Appender::builder().build("stdout", Box::new(stdout)));
        root = root.appender("stdout");
    }
    if let Some(logfile) = logfile {
        config = config.appender(Appender::builder().build("logfile", Box::new(logfile)));
        root = root.appender("logfile");
    }

    let config = config
        .build(root.build(settings.log.level_filter))
        .expect("Can't build log config!");

    log4rs::init_config(config).expect("Can't init log config!");
}

#[test]
fn test() {
    use log::{debug, error, info, trace, warn};
    init(&Settings::default());
    trace!("trace test");
    debug!("debug test");
    info!("info test");
    warn!("warn test");
    error!("error test");
}
