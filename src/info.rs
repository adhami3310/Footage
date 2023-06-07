use std::process::Command;

use itertools::Itertools;

pub fn get_width_height(path: String) -> Option<(usize, usize)> {
    let o = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(["-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height"])
        .args(["-of", "csv=s=x:p=0"])
        .arg(path)
        .output().unwrap();

    let s = std::str::from_utf8(&o.stdout).unwrap();
    
    match s.split('x').collect_vec()[..] {
        [a, b] => {
            Some((a.trim().parse().unwrap(), b.trim().parse().unwrap()))
        }
        _ => None
    }

}
