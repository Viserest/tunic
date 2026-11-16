mod calculus;

#[derive(PartialEq, Eq)]
enum Token {
    // Numbers and variables
    Number(i64),
    Variable(String),
    // Relational operators
    Equals,
    GreaterThan,
    LessThan,
    GreaterThanOrEqualTo,
    LessThanOrEqualTo,
    // Operators
    Addition,
    Subtraction,
    Multiplication,
    Division,
    // Segments
    Segment(Vec<Token>),
}

enum State {
    Continue,
    InVariable,
}

fn parser(input: String) -> Vec<Token> {
    let mut a = Vec::new();
    let mut state = State::Continue;
    for c in input.chars() {
        match state {
            State::Continue => {
                if c.is_ascii() {
                    state = State::InVariable;
                    a.push(Token::Variable(c.to_string()));
                }
            }
            State::InVariable => {
                if !c.is_ascii() {
                    state = State::Continue;
                }
                match a.last_mut().unwrap() {
                    Token::Variable(a) => a.push(c),
                    _ => {}
                }
            }
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_simple() {
        let a = "1 + 1".to_string();
        let b = parser(a);
        let mut c = b.iter();
        assert!(*c.next().unwrap() == Token::Number(1));
        assert!(*c.next().unwrap() == Token::Addition);
        assert!(*c.next().unwrap() == Token::Number(1));
    }
}
