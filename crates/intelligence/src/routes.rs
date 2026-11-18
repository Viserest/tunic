use std::collections::HashMap;

use router::{Request, Response, Router};
use utils::{Value, json_obj};

use crate::{MathError, integrate, limit, parse};

fn arg<'a>(req: &'a Request, body: &'a Value, name: &str) -> Option<String> {
    req.query_param(name).map(String::from).or_else(|| body.get(name).and_then(|v| v.as_str().map(String::from).or_else(|| v.as_f64().map(|f| f.to_string()))))
}

fn num(req: &Request, body: &Value, name: &str) -> Result<f64, Response> {
    let s = arg(req, body, name).ok_or_else(|| Response::bad_request(format!("`{name}` is required")))?;
    // numbers may themselves be expressions such as `pi/2`
    parse(&s)
        .and_then(|e| e.eval(&HashMap::new()))
        .map_err(|e| Response::bad_request(format!("`{name}`: {e}")))
}

fn fail(e: MathError) -> Response {
    Response::bad_request(e.to_string())
}

/// `GET|POST /intelligence/calc|derive|integrate|limit` with `expr` (query string or json body).
/// Extra variables for `calc` are passed as further parameters (`?expr=x*y&x=2&y=3`).
pub fn router() -> Router {
    let wrap = |f: fn(&Request, &Value) -> Response| {
        move |req: &Request| {
            let body = if req.body.is_empty() { Value::object() } else { req.json().unwrap_or_else(|_| Value::object()) };
            f(req, &body)
        }
    };
    let calc = wrap(|req, body| {
        let Some(src) = arg(req, body, "expr") else { return Response::bad_request("`expr` is required") };
        let e = match parse(&src) {
            Ok(e) => e,
            Err(e) => return fail(e),
        };
        let mut vars = HashMap::new();
        for (k, v) in req.query.iter().filter(|(k, _)| k != "expr") {
            if let Ok(n) = v.parse::<f64>() {
                vars.insert(k.clone(), n);
            }
        }
        if let Some(o) = body.get("vars").and_then(|v| v.as_object()) {
            for (k, v) in o {
                if let Some(n) = v.as_f64() {
                    vars.insert(k.clone(), n);
                }
            }
        }
        match e.eval(&vars) {
            Ok(v) => Response::json(&json_obj! {"expr" => e.to_string(), "result" => v}),
            Err(e) => fail(e),
        }
    });
    let derive = wrap(|req, body| {
        let Some(src) = arg(req, body, "expr") else { return Response::bad_request("`expr` is required") };
        let var = arg(req, body, "var").unwrap_or_else(|| "x".into());
        match parse(&src).and_then(|e| e.derivative(&var)) {
            Ok(d) => Response::json(&json_obj! {"var" => var, "derivative" => d.to_string()}),
            Err(e) => fail(e),
        }
    });
    let integ = wrap(|req, body| {
        let Some(src) = arg(req, body, "expr") else { return Response::bad_request("`expr` is required") };
        let var = arg(req, body, "var").unwrap_or_else(|| "x".into());
        let (a, b) = match (num(req, body, "from"), num(req, body, "to")) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(r), _) | (_, Err(r)) => return r,
        };
        match parse(&src).and_then(|e| integrate(&e, &var, a, b)) {
            Ok(v) => Response::json(&json_obj! {"var" => var, "from" => a, "to" => b, "result" => v}),
            Err(e) => fail(e),
        }
    });
    let lim = wrap(|req, body| {
        let Some(src) = arg(req, body, "expr") else { return Response::bad_request("`expr` is required") };
        let var = arg(req, body, "var").unwrap_or_else(|| "x".into());
        let at = match num(req, body, "at") {
            Ok(a) => a,
            Err(r) => return r,
        };
        match parse(&src).and_then(|e| limit(&e, &var, at)) {
            Ok(v) => Response::json(&json_obj! {"var" => var, "at" => at, "result" => v}),
            Err(e) => fail(e),
        }
    });
    Router::new()
        .any("/intelligence/calc", calc)
        .any("/intelligence/derive", derive)
        .any("/intelligence/integrate", integ)
        .any("/intelligence/limit", lim)
        .tagged("public")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &str) -> Value {
        router().handle(Request::get(path)).json_body().unwrap()
    }

    #[test]
    fn endpoints() {
        assert_eq!(get("/intelligence/calc?expr=2%2B3*x&x=4").get("result").unwrap().as_f64(), Some(14.0));
        assert_eq!(get("/intelligence/derive?expr=x%5E2").get("derivative").unwrap().as_str(), Some("2*x"));
        let i = get("/intelligence/integrate?expr=x%5E2&from=0&to=3");
        assert!((i.get("result").unwrap().as_f64().unwrap() - 9.0).abs() < 1e-8);
        assert_eq!(get("/intelligence/limit?expr=sin(x)%2Fx&at=0").get("result").unwrap().as_f64(), Some(1.0));
        let pi = get("/intelligence/integrate?expr=sin(x)&from=0&to=pi");
        assert!((pi.get("result").unwrap().as_f64().unwrap() - 2.0).abs() < 1e-8);
    }

    #[test]
    fn bad_input_is_a_400_not_a_crash() {
        let r = router();
        assert_eq!(r.handle(Request::get("/intelligence/calc")).status, 400);
        assert_eq!(r.handle(Request::get("/intelligence/calc?expr=1%2F0")).status, 400);
        assert_eq!(r.handle(Request::get("/intelligence/integrate?expr=x&from=a")).status, 400);
        let post = r.handle(Request::post("/intelligence/calc", r#"{"expr":"x*y","vars":{"x":2,"y":3}}"#));
        assert_eq!(post.json_body().unwrap().get("result").unwrap().as_f64(), Some(6.0));
    }
}
