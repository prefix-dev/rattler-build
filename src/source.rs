pub enum Checksum {
    sha256 {
        value : String
    },
    md5 {
        value : String
    }
}

pub enum Source {
    git {
        git_src : String,
        git_rev : String,
        git_depth : u32
    },
    url {
        url : String
    },
}

fn url_src(source : Source) {
    
}

fn git_src(source : Source) {

}

pub fn fetch_sources(sources : &Vec<Source>) {

}