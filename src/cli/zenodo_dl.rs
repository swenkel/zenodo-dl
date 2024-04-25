use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use zenodo_dl_core::download_record;


/// Simple cli program to download all files from a Zenodo record
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Arguments {
    /// Zenodo record id
    #[arg(short, long)]
    record_id: String,

    /// Output folder
    #[arg(short, long)]
    output_folder: String,

    /// create output folder if not exists
    #[clap(default_value_t = true)]
    #[arg(short, long)]
    create_output_folder: bool,

    /// Continue on error
    #[clap(default_value_t = false)]
    #[arg(short, long)]
    abort_on_error: bool
}

#[tokio::main]
async fn main() ->  ExitCode {
    let mut return_code: ExitCode = ExitCode::from(1);

    let args = Arguments::parse();

    let out_path = Path::new(&args.output_folder);

    let mut out_path_ok: bool = false;
    
    if  out_path.exists() && out_path.is_dir() {
        out_path_ok = true;
    } else if !out_path.exists() && args.create_output_folder {
        out_path_ok = match fs::create_dir_all(out_path) {
            Ok(_) => true,
            Err(_) => { println!("failed to create output folder"); false }
        };
    } else {
        println!("An error occurred!");
        println!("Target path exists: {}", out_path.exists());
        println!("Target path is folder: {}", out_path.is_dir());
    }

    if out_path_ok {
        let error_encoutered: bool = download_record(
            &args.record_id, &args.output_folder,
            &args.abort_on_error).await;
        if !error_encoutered {
            return_code = ExitCode::SUCCESS;
        }
    }
    return return_code;
}
