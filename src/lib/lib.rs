use std::{io, path::Path, fs};
use std::io::Write;

use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{self};
use futures_util::StreamExt;
use serde::{Serialize, Deserialize};
use md5::{Md5, Digest};


const ZENODO_API_BASE_URL: &str  = "https://zenodo.org/api/records/";
const ZENODO_API_BASE_URL_SUFFIX: &str  = "/files";


#[derive(Serialize, Deserialize, Debug)]
struct Links {
    content: String,
    #[serde(rename = "self")]
    links_self: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DataEntry {
    bucket_id: String,
    checksum: String,
    created: String,
    file_id: String,
    key: String,
    links: Links,
    metadata: Option<String>,
    mimetype: String,
    size: u64,
    status: String,
    storage_class: String,
    updated: String,
    version_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ZenodoMetaData {
    enabled: bool,
    entries: Option<Vec<DataEntry>>,
}

struct FileData {
    filename: String,
    // checksum_type: String,
    checksum: String,
    url: String,
    size: u64,
}

struct FileList {
    data_available: bool,
    file_list:Vec<FileData>,
}


fn verify_checksum(file: &mut fs::File, checksum: &str) -> bool
{
    let file_ok:bool;
    let mut hasher = Md5::new();
    let bytes_read = io::copy(file, &mut hasher);
    if bytes_read.is_ok() {
        let hash_bytes = hasher.finalize();
        let hash_str: String = format!("{:02x}", hash_bytes);
        file_ok = checksum == hash_str;
    } else {
        file_ok = false;
    }
    
    return file_ok
}


fn check_existing_file(filepath: &Path, filename: &str, checksum: &str) -> bool
{
    let mut skip: bool = false;
    
    if filepath.exists() && filepath.is_file() {
        match fs::File::open(&filepath) {
            Ok(mut file) => {
                let file_ok: bool = verify_checksum(&mut file, &checksum);
                if !file_ok {
                    skip = match fs::remove_file(&filepath) {
                        Ok(_) => {
                            println!("incorrect checksum - deleted {} - attempt new download", &filename);
                            false                            
                        },
                        Err(_) => { 
                            println!("incorrect checksum - failed to delete {} - skipping file", &filename);
                            true
                        }
                    };
                } else {
                    println!("{} downloaded already - skipping file", &filename);
                    skip = true;
                }
            },
            Err(_) => ()
        };
    }
    return skip;
}


async fn download_file(filepath: &Path, filename: &str, url: &str,
    checksum: &str, filesize: u64) -> Result<bool, String>
{
    // let mut success: bool = false;
    let res = reqwest::get(url).await.or(Err("bla"))?;

    // todo
    //     - proper graceful error handling for this progress bar
    //       (no progress bar for whatever reason is no reason for not downloading)
    
    let pb = ProgressBar::new(filesize);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.green/green}] {bytes}/{total_bytes} ({bytes_per_sec} [eta: {eta}])")
        .or(Err("can't create progress bar"))?
        .progress_chars("#>-"));

    let mut output_file = fs::File::create(filepath).or(
        Err(format!("Could not create {}", &filename)))?;
    let mut bytes_downloaded: u64 = 0u64;
    println!("Downloading {}", &filename);
    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = item.unwrap();
        output_file.write_all(&chunk).or(Err("Error writing to file - check your disk space"))?;
        bytes_downloaded = std::cmp::min(bytes_downloaded + (chunk.len() as u64), filesize);
        pb.set_position(bytes_downloaded);
    }
    pb.finish();


    output_file.flush().or(Err(format!("Could not flush remaining bytes to {}", &filename)))?;

    // close file by dropping out of scope
    drop(output_file);

    let success: bool = match fs::File::open(&filepath) {
        Ok(mut output_file) => verify_checksum(&mut output_file, &checksum),
        Err(_) => false
        };
    if !success {
        println!("checksum of {} does not match - deleting file", &filename);
        fs::remove_file(&filepath).or(Err(
            format!("failed to remove {}", &filename)))?;
    }

    return Ok(success);
}


async fn download_files(files: &Vec<FileData>,
    target_folder: &str, abort_on_error: &bool) -> bool
{
    let mut error_encountered = false;
    for entry in files.iter()
    {
        if !error_encountered {
            let filepath = Path::new(target_folder).join(&entry.filename);
            let skip: bool = check_existing_file(&filepath, &entry.filename,
                &entry.checksum);
            if !skip {
                let resp_ok: bool = match download_file(&filepath, &entry.filename, &entry.url, &entry.checksum, entry.size).await {
                    Ok(success) => success,
                    Err(_) => false
                };
                if !resp_ok {
                    error_encountered = true;
                    if *abort_on_error {
                        break;
                    }
                }
            }
        }
    }
    return error_encountered;
}

async fn parse_json_response(resp: reqwest::Response, error: &mut bool) -> ZenodoMetaData
{
    let dummy_response: ZenodoMetaData = ZenodoMetaData {
        enabled: false,
        entries: None,
    };
    let meta_data_received: ZenodoMetaData;
    if resp.status() == 200u16 {
        let bla: ZenodoMetaData = match &resp.text().await {
            Ok(body) => { *error = false; match serde_json::from_str(&body) {
                Ok(parsed) => { *error = false; parsed },
                Err(_) => { *error = true; dummy_response }}} ,
            Err(_) => { *error = true; dummy_response }
        };
        meta_data_received = bla;
    } else {
        meta_data_received = dummy_response;
    }

    return meta_data_received;
}

async fn download_record_meta(record_id: &str) -> ZenodoMetaData
{
    let url: String = ZENODO_API_BASE_URL.to_string() + 
        record_id + ZENODO_API_BASE_URL_SUFFIX;
    
    let mut error: bool = true;
    let dummy_response: ZenodoMetaData = ZenodoMetaData {
        enabled: false,
        entries: None,
    };

    let meta_data_received: ZenodoMetaData = match reqwest::get(&url).await {
        Ok(res) => { parse_json_response(res, &mut error).await },
        Err(_) => { error = true; dummy_response }
    };

    if error {
        println!("An error occurred! Check the record ID before retry.");
    }

    return meta_data_received;    
}

fn create_file_list(meta_data: &ZenodoMetaData) ->FileList
{
    let empty_response: FileList = FileList {
        data_available: false,
        file_list: vec![FileData {
            filename: "empty".to_string(),
            // checksum_type: "empty".to_string(),
            checksum: "empty".to_string(),
            url: "empty".to_string(),
            size: 0u64,
        }]
    };

    let file_list: FileList;
    let mut file_list_tmp: Vec<FileData> = Vec::new();
    if meta_data.enabled && meta_data.entries.is_some() {
        for entry in meta_data.entries.iter().flatten()
        {
            let start_pos_checksum: usize = entry.checksum.find(":").unwrap_or(0);

            file_list_tmp.push(FileData {
                filename: entry.key.clone(),
                // checksum_type: entry.checksum[..start_pos_checksum].to_string(),
                checksum: entry.checksum[start_pos_checksum+1..].to_string(),
                url: entry.links.content.clone(),
                size: entry.size,
            });
        }
    }

    if !file_list_tmp.is_empty() {
        file_list = FileList {
            data_available: true,
            file_list: file_list_tmp,
        };
    } else {
        file_list = empty_response;
    }    

    return file_list;
}

pub async fn download_record(record_id: &str, target_folder: &str,
    abort_on_error: &bool) -> bool
{
    let mut error_encountered: bool = false;
    let meta_data: ZenodoMetaData = download_record_meta(record_id).await;
    let file_list: FileList = create_file_list(&meta_data);

    if file_list.data_available {
        error_encountered = download_files(&file_list.file_list,
            &target_folder, &abort_on_error).await;
    }
    return error_encountered
}
