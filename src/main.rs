/*
 * jail_exporter
 * -------------
 *
 * An exporter for Prometheus, exporting jail metrics as reported by rctl(8).
 *
 */
extern crate env_logger;
extern crate hyper;
extern crate libc;
#[macro_use] extern crate clap;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
#[macro_use] extern crate prometheus;

mod jail;
mod rctl;

use hyper::{
    Body,
    Method,
    Request,
    Response,
    Server,
    StatusCode,
};
use hyper::header::CONTENT_TYPE;
use hyper::rt::Future;
use hyper::service::service_fn_ok;
use prometheus::{
    Encoder,
    IntCounterVec,
    IntGauge,
    IntGaugeVec,
    TextEncoder,
};
use std::io::Error;
use std::net::SocketAddr;
use std::process::exit;
use std::str::FromStr;

// Descriptions of these metrics are taken from rctl(8) where possible.
lazy_static!{
    // build info metric
    static ref JAIL_EXPORTER_BUILD_INFO: IntGaugeVec = register_int_gauge_vec!(
        "jail_exporter_build_info",
        "A metric with a constant '1' value labelled by version \
         from which jail_exporter was built",
        &["version"]
    ).unwrap();

    // Bytes metrics
    static ref JAIL_COREDUMPSIZE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_coredumpsize_bytes",
        "core dump size, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_DATASIZE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_datasize_bytes",
        "data size, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_MEMORYLOCKED_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_memorylocked_bytes",
        "locked memory, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_MEMORYUSE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_memoryuse_bytes",
        "resident set size, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_MSGQSIZE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_msgqsize_bytes",
        "SysV message queue size, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_SHMSIZE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_shmsize_bytes",
        "SysV shared memory size, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_STACKSIZE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_stacksize_bytes",
        "stack size, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_SWAPUSE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_swapuse_bytes",
        "swap space that may be reserved or used, in bytes",
        &["name"]
    ).unwrap();

    static ref JAIL_VMEMORYUSE_BYTES: IntGaugeVec = register_int_gauge_vec!(
        "jail_vmemoryuse_bytes",
        "address space limit, in bytes",
        &["name"]
    ).unwrap();

    // Percent metrics
    static ref JAIL_PCPU_USED: IntCounterVec = register_int_counter_vec!(
        "jail_pcpu_used",
        "%CPU, in percents of a single CPU core",
        &["name"]
    ).unwrap();

    // Random numberical values without a specific unit.
    static ref JAIL_MAXPROC: IntGaugeVec = register_int_gauge_vec!(
        "jail_maxproc",
        "number of processes",
        &["name"]
    ).unwrap();

    static ref JAIL_MSGQQUEUED: IntGaugeVec = register_int_gauge_vec!(
        "jail_msgqqueued",
        "number of queued SysV messages",
        &["name"]
    ).unwrap();

    static ref JAIL_NMSGQ: IntGaugeVec = register_int_gauge_vec!(
        "jail_nmsgq",
        "number of SysV message queues",
        &["name"]
    ).unwrap();

    static ref JAIL_NSEM: IntGaugeVec = register_int_gauge_vec!(
        "jail_nsem",
        "number of SysV semaphores",
        &["name"]
    ).unwrap();

    static ref JAIL_NSEMOP: IntGaugeVec = register_int_gauge_vec!(
        "jail_nsemop",
        "number of SysV semaphores modified in a single semop(2) call",
        &["name"]
    ).unwrap();

    static ref JAIL_NSHM: IntGaugeVec = register_int_gauge_vec!(
        "jail_nshm",
        "number of SysV shared memory segments",
        &["name"]
    ).unwrap();

    static ref JAIL_NTHR: IntGaugeVec = register_int_gauge_vec!(
        "jail_nthr",
        "number of threads",
        &["name"]
    ).unwrap();

    static ref JAIL_OPENFILES: IntGaugeVec = register_int_gauge_vec!(
        "jail_openfiles",
        "file descriptor table size",
        &["name"]
    ).unwrap();

    static ref JAIL_PSEUDOTERMINALS: IntGaugeVec = register_int_gauge_vec!(
        "jail_pseudoterminals",
        "number of PTYs",
        &["name"]
    ).unwrap();

    // Seconds metrics
    static ref JAIL_CPUTIME_SECONDS: IntCounterVec = register_int_counter_vec!(
        "jail_cputime_seconds_total",
        "CPU time, in seconds",
        &["name"]
    ).unwrap();

    static ref JAIL_WALLCLOCK_SECONDS: IntCounterVec = register_int_counter_vec!(
        "jail_wallclock_seconds_total",
        "wallclock time, in seconds",
        &["name"]
    ).unwrap();

    // Metrics created by the exporter
    static ref JAIL_ID: IntGaugeVec = register_int_gauge_vec!(
        "jail_id",
        "ID of the named jail.",
        &["name"]
    ).unwrap();

    static ref JAIL_TOTAL: IntGauge = register_int_gauge!(
        "jail_num",
        "Current number of running jails."
    ).unwrap();
}

// Processes the MetricsHash setting the appripriate time series.
fn process_metrics_hash(name: &str, metrics: &rctl::MetricsHash) {
    debug!("process_metrics_hash");

    for (key, value) in metrics {
        match key.as_ref() {
            "coredumpsize" => {
                JAIL_COREDUMPSIZE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "cputime" => {
                let series = JAIL_CPUTIME_SECONDS.with_label_values(&[&name]);
                let inc = *value - series.get();
                series.inc_by(inc);
            },
            "datasize" => {
                JAIL_DATASIZE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "maxproc" => {
                JAIL_MAXPROC.with_label_values(&[&name]).set(*value);
            },
            "memorylocked" => {
                JAIL_MEMORYLOCKED_BYTES.with_label_values(&[&name]).set(*value);
            },
            "memoryuse" => {
                JAIL_MEMORYUSE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "msgqqueued" => {
                JAIL_MSGQQUEUED.with_label_values(&[&name]).set(*value);
            },
            "msgqsize" => {
                JAIL_MSGQSIZE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "nmsgq" => {
                JAIL_NMSGQ.with_label_values(&[&name]).set(*value);
            },
            "nsem" => {
                JAIL_NSEM.with_label_values(&[&name]).set(*value);
            },
            "nsemop" => {
                JAIL_NSEMOP.with_label_values(&[&name]).set(*value);
            },
            "nshm" => {
                JAIL_NSHM.with_label_values(&[&name]).set(*value);
            },
            "nthr" => {
                JAIL_NTHR.with_label_values(&[&name]).set(*value);
            },
            "openfiles" => {
                JAIL_OPENFILES.with_label_values(&[&name]).set(*value);
            },
            "pcpu" => {
                JAIL_PCPU_USED.with_label_values(&[&name]).inc_by(*value);
            },
            "pseudoterminals" => {
                JAIL_PSEUDOTERMINALS.with_label_values(&[&name]).set(*value);
            },
            "shmsize" => {
                JAIL_SHMSIZE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "stacksize" => {
                JAIL_STACKSIZE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "swapuse" => {
                JAIL_SWAPUSE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "vmemoryuse" => {
                JAIL_VMEMORYUSE_BYTES.with_label_values(&[&name]).set(*value);
            },
            "wallclock" => {
                let series = JAIL_WALLCLOCK_SECONDS.with_label_values(&[&name]);
                let inc = *value - series.get();
                series.inc_by(inc);
            },
            // jid isn't actually reported by rctl, but we add it into this
            // hash to keep things simpler.
            "jid" => {
                JAIL_ID.with_label_values(&[&name]).set(*value);
            },
            // Intentionally unhandled metrics.
            // These are documented being difficult to observe via rctl(8).
            "readbps" | "writebps" | "readiops" | "writeiops" => {},
            _ => println!("Unrecognised metric: {}", key),
        }
    }
}

fn get_jail_metrics() {
    debug!("get_jail_metrics");
    let mut lastjid = 0;

    // Set JAIL_TOTAL to zero before gathering.
    JAIL_TOTAL.set(0);

    // Loop over jails.
    while lastjid >= 0 {
        let (jid, value) = jail::get(lastjid, "name");
        debug!("JID: {}, Name: {:?}", jid, value);

        if jid > 0 {
            let name = match value {
                Some(value) => value,
                None => "".to_string(),
            };

            let rusage = match rctl::get_resource_usage(jid, &name) {
                Ok(res) => res,
                Err(err) => {
                    err.to_string();
                    break;
                },
            };

            // Get a hash of resources based on rusage string.
            process_metrics_hash(&name, &rusage);

            JAIL_TOTAL.set(JAIL_TOTAL.get() + 1);
        }
        else {
            // Lastjid was never changed and jail_get returned < -1
            // Some error other than not finding jails occurred
            if lastjid == 0 && jid < -1 {
                println!("{:?}", Error::last_os_error());
            }
            // lastjid was changed and jid is -1
            // We successfully interated over jails and none are left.
            else if lastjid != 0 && jid == -1 {

            }
            else {
                println!("No jails found");
            }
        }

        lastjid = jid;
    }
}

fn metrics(_req: Request<Body>) -> Response<Body> {
    debug!("Processing metrics request");

    get_jail_metrics();
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain")
        .body(Body::from(buffer))
        .unwrap()
}

// HTTP request router
fn http_router(req: Request<Body>) -> Response<Body> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/metrics") => {
            metrics(req)
        },
        _ => {
            debug!("No handler for request found");
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap()
        },
    }
}

// Used as a validator for the argument parsing.
fn is_ipaddress(s: String) -> Result<(), String> {
    let res = SocketAddr::from_str(&s);
    match res {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("'{}' is not a valid ADDR:PORT string", s)),
    }
}

fn main() {
    env_logger::init();

    // First, check if RACCT/RCTL is available.
    debug!("Checking RACCT/RCTL status");
    let racct_rctl_available = match rctl::is_enabled() {
        rctl::State::Disabled => {
            eprintln!("RACCT/RCTL present, but disabled; enable using \
                      kern.racct.enable=1 tunable");
            false
        },
        rctl::State::Enabled => {
            true
        },
        rctl::State::NotPresent => {
            eprintln!("RACCT/RCTL support not present in kernel; see rctl(8) \
                      for details");
            false
        },
        rctl::State::UnknownError(s) => {
            eprintln!("Unknown error while checking RACCT/RCTL state: {}", s);
            false
        },
    };

    // If it's not available, exit.
    if !racct_rctl_available {
        exit(1);
    }

    debug!("Parsing command line arguments");
    let matches = clap::App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(clap::Arg::with_name("WEB_LISTEN_ADDRESS")
             .long("web.listen-address")
             .value_name("[ADDR:PORT]")
             .help("Address on which to expose metrics and web interface.")
             .takes_value(true)
             .default_value("127.0.0.1:9999")
             .validator(is_ipaddress))
        .arg(clap::Arg::with_name("WEB_TELEMETRY_PATH")
             .long("web.telemetry-path")
             .value_name("PATH")
             .help("Path under which to expose metrics.")
             .takes_value(true)
             .default_value("/metrics"))
        .get_matches();

    // This should always be fine, we've already validated it during arg
    // parsing.
    // However, we keep the expect as a last resort.
    let addr: SocketAddr = matches.value_of("WEB_LISTEN_ADDRESS").unwrap()
        .parse().expect("unable to parse socket address");

    let router = || {
        service_fn_ok(http_router)
    };

    // Set build_info metric.
    let build_info_labels = [
        crate_version!(),
    ];

    JAIL_EXPORTER_BUILD_INFO.with_label_values(&build_info_labels).set(1);

    info!("Starting HTTP server on {}", addr);
    let server = Server::bind(&addr)
        .serve(router)
        .map_err(|e| eprintln!("server error: {}", e));

    hyper::rt::run(server);
}
