error_chain! {
    errors {
        InvalidParameterValue(pname: String) {
            description("An invalid value was supplied for a command line argument.")
            display("Invalid value supplied for parameter {}", pname)
        }
        InvalidUsage
        InvalidIncludeFilter
    }
    foreign_links {
        Io(::std::io::Error);
    }
}
