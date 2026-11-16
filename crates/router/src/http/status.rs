crate::status_code! {
    pub StatusCode<u16>,
    is_informational {
        // Informational
        Continue, 100, "Continue";
        SwitchingProtocols, 101, "Switching Protocols";
        Processing, 102, "Processing";
        EarlyHints, 103, "Early Hints";
    },
    is_success {
        // Successful
        Ok, 200, "OK";
        Created, 201, "Created";
        Accepted, 202, "Accepted";
        NonAuthoritativeInformation, 203, "Non-Authoritative Information";
        ContentNone, 204, "No Content";
        ContentReset, 205, "Reset Content";
        ContentPartial, 206, "Partial Content";
        MultiStatus, 207, "Multi-Status";
        AlreadyReported, 208, "Already Reported";
        IMUsed, 226, "IM Used";
    },
    is_redirection {
        // Redirection
        MultipleChoices, 300, "Multiple Choices";
        MovedPermanently, 301, "Moved Permanently";
        Found, 302, "Found";
        SeeOther, 303, "See Other";
        NotModified, 304, "Not Modified";
        UseProxy, 305, "Use Proxy";
        RedirectTemporary, 307, "Temporary Redirect";
        RedirectPermanent, 308, "Permanent Redirect";
    },
    is_client_error {
        // Client error
        BadRequest, 400, "Bad Request";
        Unauthorized, 401, "Unauthorized";
        PaymentRequired, 402, "Payment Required";
        Forbidden, 403, "Forbidden";
        NotFound, 404, "Not Found";
        MethodNotAllowed, 405, "Method Not Allowed";
        NotAcceptable, 406, "Not Acceptable";
        ProxyAuthenticationRequired, 407, "Proxy Authentication Required";
        RequestTimeout, 408, "Request Timeout";
        Conflict, 409, "Conflict";
        Gone, 410, "Gone";
        LengthRequired, 411, "Length Required";
        PreconditionFailed, 412, "Precondition Failed";
        ContentTooLarge, 413, "Content Too Large";
        URITooLong, 414, "URI Too Long";
        UnsupportedMediaType, 415, "Unsupported Media Type";
        RangeNotSatisfiable, 416, "Range Not Satisfiable";
        ExpectationFailed, 417, "Expection Failed";
        ImATeapot, 418, "Im A Teapot";
        MisdirectedRequest, 421, "Misdirected Request";
        UnprocessableContent, 422, "Unprocessable Content";
        Locked, 423, "Locked";
        FailedDependency, 424, "Failed Dependency";
        TooEarly, 425, "Too Early";
        UpgradeRequired, 426, "Upgrade Required";
        PreconditionRequired, 428, "Precondition Required";
        TooManyRequests, 429, "Too Many Requests";
        RequestHeaderFieldsTooLarge, 431, "Request Header Fields Too Large";
        UnavaiableForLegalReasons, 451, "Unavailable For Legal Reasons";
    },
    is_server_error {
        // Server error
        InternalServerError, 500, "Internal Server Error";
        NotImplemented, 501, "Not Implemented";
        BadGateway, 502, "Bad Gateway";
        ServiceUnavailable, 503, "Service Unavailable";
        GatewayTimeout, 504, "Gateway Timeout";
        HTTPVersionNotSupported, 505, "HTTP Version Not Supported";
        VariantAlsoNegotiates, 506, "Variant Also Negotiates";
        InsufficientStorage, 507, "Insufficient Storage";
        LoopDetected, 508, "Loop Detected";
        NotExtended, 510, "Not Extended";
        NetworkAuthenticationRequired, 511, "Network Authentication Required";
    },
}
