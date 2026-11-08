#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code, unnecessary_transmutes, clippy::all)]
include!(concat!(env!("OUT_DIR"), "/openexr.rs"));

pub mod tj {
    #![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/turbojpeg.rs"));
}

pub mod tiff {
    #![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/libtiff.rs"));
}
