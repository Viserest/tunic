crate::status_code! {
    pub StatusCode<u8>,
    is_input {
        // Input expected
        Input, 10, "Input";
        InputSensitive, 11, "Input Sensitive";
    },
    is_success {
        // Success
        Success, 20, "Success";
    },
    is_redirection {
        // Redirection
        RedirectTemporary, 30, "Temporary Redirect";
        RedirectPermanent, 31, "Permanent Redirect";
    },
    is_failure_temp {
        // Temporary failure
        FailureTemporary, 40, "Temporary Failure";
        ServerUnavailable, 41, "Server Unavailable";
        ErrorCGI, 42, "CGI Error";
        ErrorProxy, 43, "Proxy Error";
        SlowDown, 44, "Slow Down";
    },
    is_failure_perm {
        // Permanent failure
        FailurePermanent, 50, "Permanent Failure";
        NotFound, 51, "Not Found";
        Gone, 52, "Gone";
        ProxyRequestRefused, 53, "Proxy Request Refused";
        BadRequest, 59, "Bad Request";
    },
    is_client_cert {
        // Client certificate
        ClientCertificateRequired, 60, "Client Certificate Required";
        CertificateNotValid, 61, "Certificate Not Valid";
    },
}

mod handle;
pub use handle::*;

mod serve;
pub use serve::*;
