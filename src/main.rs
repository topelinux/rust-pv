extern crate bytes;

extern crate futures;
extern crate getopts;
extern crate tokio;
extern crate tokio_fs;

use bytes::BytesMut;
use futures::future::{lazy, loop_fn, Loop};
use getopts::Options;
use std::{env, fs, io, str::FromStr, time::Instant};

use tokio::{fs::File, prelude::*};

use tokio_fs::{stdin, stdout, Stdin};

enum FileMode {
    InputStdin(Stdin),
    InputFile(File),
}

fn usage(opts: Options) {
    let brief = ("Usage: pv [options] <OUTFILE>").to_string();
    print!("{}", opts.usage(&brief));
}

struct Pv {
    bs: usize,
    thresh_millis: u128,
    processed: u64,
    millis_processed: u64,
    millis_elapsed: u128,
    size: u64,
}

impl Pv {
    fn new(bs: usize, thresh_millis: u128) -> Self {
        Pv {
            bs,
            thresh_millis,
            processed: 0,
            millis_processed: 0,
            millis_elapsed: 0,
            size: 0,
        }
    }

    fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    fn update_status(&mut self, processed: u64, millis_elapsed: u128) {
        self.processed += processed;
        self.millis_processed += processed;
        if millis_elapsed - self.millis_elapsed >= self.thresh_millis {
            let speed =
                (self.millis_processed as u128) / ((millis_elapsed - self.millis_elapsed) * 1000);
            let status = if self.size > 0 {
                format!(
                    "speed: {} MBytes/s processed: {} %",
                    speed,
                    100 * ((self.millis_processed + self.processed) / self.size)
                )
            } else {
                format!("speed: {} MBytes/s", speed)
            };
            self.show_progress(status);
            self.millis_elapsed = millis_elapsed;
            self.millis_processed = 0;
        }
    }

    fn show_progress<T: AsRef<str>>(&self, status: T) {
        io::stderr().flush().unwrap();
        eprint!("\r");
        // TODO How to handle extra text?
        eprint!("{:<81}", status.as_ref());
    }
}

fn main() {
    let mut opts = Options::new();
    opts.optopt("b", "blocksize", "Block size in bytes", "BS");
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(env::args().skip(1)) {
        Ok(m) => m,
        Err(f) => panic!(f.to_string()),
    };

    if matches.opt_present("h") {
        usage(opts);
        return;
    }

    let bs = match matches.opt_str("b") {
        Some(v) => usize::from_str(v.as_str()).unwrap(),
        None => 512,
    };

    let mut pv = Pv::new(bs, 200);
    let mut dbs = BytesMut::with_capacity(bs);

    let mut input_file: FileMode = if !matches.free.is_empty() {
        let input_path = matches.free[0].clone();
        let metadata = fs::metadata(input_path.as_str()).unwrap();
        if metadata.file_type().is_file() {
            pv.set_size(metadata.len());
        }
        let std_file = fs::File::open(input_path.as_str()).unwrap();
        eprintln!("input file name is {}", input_path);
        FileMode::InputFile(File::from_std(std_file))
    } else {
        FileMode::InputStdin(stdin())
    };

    let mut output = stdout();
    let now = Instant::now();
    let task = lazy(move || {
        let eof = false;
        loop_fn((eof, 0), move |(mut eof, mut readed)| {
            let poll_reader = match &mut input_file {
                FileMode::InputFile(input) => input.read_buf(&mut dbs),
                FileMode::InputStdin(input) => input.read_buf(&mut dbs),
            };
            poll_reader
                .and_then(|num| {
                    let n = match num {
                        Async::Ready(n) => n,
                        _ => panic!(),
                    };
                    readed += n;
                    if n == 0 {
                        eof = true;
                    }
                    Ok(n)
                })
                .and_then(|num| {
                    if readed == bs || eof {
                        dbs.truncate(readed);
                        output
                            .poll_write(&dbs)
                            .map(|res| match res {
                                Async::Ready(n) => {
                                    if n != readed {
                                        panic!()
                                    } else {
                                        dbs.clear();
                                    }
                                }
                                _ => panic!(),
                            })
                            .map_err(|err| eprintln!("IO error: {:?}", err))
                            .unwrap();
                    }
                    Ok(num)
                })
                .and_then(|num| {
                    pv.update_status(num as u64, now.elapsed().as_millis());
                    if readed < bs && !eof {
                        return Ok(Loop::Continue((eof, num)));
                    }
                    if eof {
                        // pb.finish();
                        return output
                            .poll_flush()
                            .and_then(|_| Ok(Loop::Break((eof, num))));
                    }
                    Ok(Loop::Continue((eof, 0)))
                })
        })
        .and_then(move |_| {
            let delta = now.elapsed().as_millis();
            eprintln!("Done! use {} msec", delta);
            Ok(())
        })
    })
    .map_err(|err| eprintln!("IO error: {:?}", err));

    tokio::run(task);
}
