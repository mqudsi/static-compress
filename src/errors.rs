error_chain! {
    errors {
        InvalidParameterValue(pname: &'static str) {
            description("An invalid value was supplied for a command line argument.")
            display("Invalid value supplied for parameter {}", pname)
        }
        InvalidUsage
        InvalidIncludeFilter
        InvalidCharactersInPath
    }
    foreign_links {
        Io(::std::io::Error);
        SystemTime(::std::time::SystemTimeError);
    }
}
