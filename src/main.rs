// Disable console on windows (https://github.com/rust-lang/rust/pull/37501)
#![windows_subsystem = "windows"]

use std::fs::{File, copy, read, create_dir_all, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;

use atty::Stream;
use chrono::Local;
use clap::{Arg, App};
use dirs::home_dir;
use log::{info, debug, error, trace};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use simplelog::{CombinedLogger, TermLogger, TerminalMode, WriteLogger, LevelFilter, Config, SharedLogger};

extern crate exitcode;

fn main() {
	// Setup CLI arguments
	let matches = App::new("file-watcher-backup")
		.about("Whenever a file changes, copy it's content to a backup file.")
		.version("0.1.0")
		.author("niktekusho <https://github.com/nikteksuho")
		.arg(Arg::with_name("source")
			.short("s")
			.long("source")
			.value_name("FILE")
			.help("Source file to watch")
			.required(true)
			.index(1)
			.takes_value(true))
		.arg(Arg::with_name("destination")
			.short("d")
			.long("destination")
			.value_name("DIR")
			.help("Target directory in which the file will be copied")
			.required(true)
			.index(2)
			.takes_value(true))
		.get_matches();

	let mut loggers: Vec<Box<SharedLogger>> = Vec::new();

	// First: configure the console logger if we have an attached terminal
	if atty::is(Stream::Stdout) {
		// Terminal
		loggers.push(TermLogger::new(LevelFilter::Debug, Config::default(), TerminalMode::Mixed).unwrap());
	}

	let file_logger = create_file_logger();
	if file_logger.is_some() {
		loggers.push(file_logger.unwrap());
	}

	CombinedLogger::init(loggers).unwrap();

	// Since "source" argument is required, unwrap() here is safe
	let src_path = matches.value_of("source").unwrap();
	debug!("Input path: `{}`", src_path);

	// Fail early if the path does not link to an existing file or
	// the user doesn't have read access to it
	match read(src_path) {
		Ok(file) => file,
		Err(error) => match error.kind() {
			ErrorKind::NotFound => {
				error!("File `{}` not found", src_path);
				trace!("{:?}", error);
				std::process::exit(exitcode::NOINPUT);
			}
			other_errors => {
				error!("Error accessing file `{}`", src_path);
				trace!("{:?}", other_errors);
				std::process::exit(exitcode::IOERR);
			}
		}
	};

	info!("Input file validated");

	let destination_dir_path = matches.value_of("destination").unwrap();
	debug!("Destination dir is: {}", destination_dir_path);

	// Handle only the error part of the result (since the value is void)
	if let Err(err) = create_dir_all(destination_dir_path) {
		debug!("{:?}", err);
		error!("Destination directory `{}` setup failed", destination_dir_path);
		std::process::exit(exitcode::IOERR);
	}

	info!("Destination dir `{}` setup completed", destination_dir_path);

	let mut _destination_file_path = PathBuf::from(destination_dir_path);
	// Here "src_path" is a confirmed file so the unwrap is secure
	_destination_file_path.push(Path::new(src_path).file_name().unwrap());

	let destination_file_path = _destination_file_path.as_path();

	// Make the first copy, just to start with a balanced state
	debug!("Initial copy of `{}` into `{:?}`", src_path, destination_file_path);
	match copy(src_path, destination_file_path) {
		Ok(filesize) => debug!("Copied {} bytes", filesize),
		Err(error) => {
			debug!("{:?}", error);
			error!("First copy failed:. Reason: {}", error);
		}
	};

	let (tx, rx) = channel();
	let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(1)).unwrap();
	match watcher.watch(src_path, RecursiveMode::NonRecursive) {
		Ok(()) => (),
		Err(error) => error!("Error adding path to watcher. {:?}", error)
	};

	loop {
		match rx.recv() {
			Ok(event) => {
				match event {
					notify::DebouncedEvent::Write(path) => {
						match copy(path, destination_file_path) {
							Ok(filesize) => debug!("Copied {} bytes", filesize),
							Err(error) => {
								debug!("{:?}", error);
								error!("First copy failed:. Reason: {}", error);
							}
						};
					},
					_ => continue
				}
			},
			Err(e) => error!("Watch error. {:?}", e)
		}
	}
}

fn create_file_logger() -> Option<Box<WriteLogger<File>>> {
	// The default log directory for the moment is the $HOME/file-watcher-backup directory of the user
	let mut _log_path = match home_dir() {
		Some(path) => path,
		None => return None
	};

	_log_path.push("file-watcher-backup");
	match create_dir_all(&_log_path) {
		Ok(()) => (),
		Err(_) => return None
	};

	let log_file_name = format!("{}.log", Local::now().format("%Y-%m-%d"));
	_log_path.push(log_file_name);

	let log_file_path = _log_path.as_path();

	match OpenOptions::new().create(true).append(true).open(log_file_path) {
			Ok(file) => Some(WriteLogger::new(LevelFilter::Trace, Config::default(), file)),
			Err(_) => return None
	}
}
