//! LaTeX-subset syntax tree and bounded parser.

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Node {
    /// A character run: identifiers italic, numbers and symbols upright.
    Sym(String, Class),
    Group(Vec<Node>),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Option<Box<Node>>, Box<Node>),
    Scripts {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    Text(String),
    Space(f64),
    /// A big operator (∑ ∏ ∫): drawn larger, limits as scripts.
    Big(String),
    /// `\left( … \right)`: delimiters sized to the content.
    Fenced(String, Box<Node>, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Class {
    Ident,
    Number,
    /// Binary operator (+ − × ·): medium space both sides.
    Bin,
    /// Relation (= < ≤ →): thick space both sides.
    Rel,
    Punct,
    Plain,
}

fn symbol(name: &str) -> Option<(&'static str, Class)> {
    use Class::*;
    Some(match name {
        "alpha" => ("α", Ident),
        "beta" => ("β", Ident),
        "gamma" => ("γ", Ident),
        "delta" => ("δ", Ident),
        "epsilon" | "varepsilon" => ("ε", Ident),
        "zeta" => ("ζ", Ident),
        "eta" => ("η", Ident),
        "theta" => ("θ", Ident),
        "iota" => ("ι", Ident),
        "kappa" => ("κ", Ident),
        "lambda" => ("λ", Ident),
        "mu" => ("μ", Ident),
        "nu" => ("ν", Ident),
        "xi" => ("ξ", Ident),
        "pi" => ("π", Ident),
        "rho" => ("ρ", Ident),
        "sigma" => ("σ", Ident),
        "tau" => ("τ", Ident),
        "phi" | "varphi" => ("φ", Ident),
        "chi" => ("χ", Ident),
        "psi" => ("ψ", Ident),
        "omega" => ("ω", Ident),
        "Gamma" => ("Γ", Plain),
        "Delta" => ("Δ", Plain),
        "Theta" => ("Θ", Plain),
        "Lambda" => ("Λ", Plain),
        "Pi" => ("Π", Plain),
        "Sigma" => ("Σ", Plain),
        "Phi" => ("Φ", Plain),
        "Psi" => ("Ψ", Plain),
        "Omega" => ("Ω", Plain),
        "times" => ("×", Bin),
        "cdot" => ("·", Bin),
        "pm" => ("±", Bin),
        "mp" => ("∓", Bin),
        "div" => ("÷", Bin),
        "le" | "leq" => ("≤", Rel),
        "ge" | "geq" => ("≥", Rel),
        "neq" | "ne" => ("≠", Rel),
        "approx" => ("≈", Rel),
        "equiv" => ("≡", Rel),
        "to" | "rightarrow" => ("→", Rel),
        "leftarrow" => ("←", Rel),
        "Rightarrow" => ("⇒", Rel),
        "in" => ("∈", Rel),
        "infty" => ("∞", Plain),
        "partial" => ("∂", Plain),
        "nabla" => ("∇", Plain),
        "ldots" | "dots" => ("…", Plain),
        "cdots" => ("⋯", Plain),
        "prime" => ("′", Plain),
        "degree" => ("°", Plain),
        _ => return None,
    })
}

fn big_op(name: &str) -> Option<&'static str> {
    Some(match name {
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "oint" => "∮",
        _ => return None,
    })
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }
    fn command(&mut self) -> String {
        let mut name = String::new();
        if let Some(c) = self.peek()
            && !c.is_ascii_alphabetic()
        {
            self.i += 1;
            return c.to_string();
        }
        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
            name.push(c);
            self.i += 1;
        }
        name
    }
    /// Raw text of a `{…}` argument.
    fn raw_group(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self
                .peek()
                .map(|c| {
                    self.i += 1;
                    c.to_string()
                })
                .unwrap_or_default();
        }
        self.i += 1;
        let mut depth = 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            out.push(c);
        }
        out
    }
    fn arg(&mut self) -> Node {
        self.skip_ws();
        if self.peek() == Some('{') {
            self.i += 1;
            let g = self.list(Some('}'));
            Node::Group(g)
        } else {
            self.atom().unwrap_or(Node::Group(vec![]))
        }
    }
    fn delimiter(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => {
                self.i += 1;
                match self.command().as_str() {
                    "{" => "{".into(),
                    "}" => "}".into(),
                    "|" => "‖".into(),
                    "langle" => "⟨".into(),
                    "rangle" => "⟩".into(),
                    _ => String::new(),
                }
            }
            Some('.') => {
                self.i += 1;
                String::new()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }
    fn atom(&mut self) -> Option<Node> {
        self.skip_ws();
        let c = self.peek()?;
        self.i += 1;
        Some(match c {
            '{' => Node::Group(self.list(Some('}'))),
            '\\' => {
                let name = self.command();
                match name.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        let a = self.arg();
                        let b = self.arg();
                        Node::Frac(Box::new(a), Box::new(b))
                    }
                    "sqrt" => {
                        self.skip_ws();
                        let idx = (self.peek() == Some('[')).then(|| {
                            self.i += 1;
                            let mut s = String::new();
                            while let Some(c) = self.peek() {
                                self.i += 1;
                                if c == ']' {
                                    break;
                                }
                                s.push(c);
                            }
                            Box::new(parse(&s))
                        });
                        let a = self.arg();
                        Node::Sqrt(idx, Box::new(a))
                    }
                    "text" | "mathrm" | "textrm" | "operatorname" => Node::Text(self.raw_group()),
                    "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "lim" | "max" | "min" => Node::Text(name),
                    "," => Node::Space(3.0 / 18.0),
                    ":" | ">" => Node::Space(4.0 / 18.0),
                    ";" => Node::Space(5.0 / 18.0),
                    "quad" => Node::Space(1.0),
                    "qquad" => Node::Space(2.0),
                    "!" => Node::Space(-3.0 / 18.0),
                    " " => Node::Space(0.25),
                    "left" => {
                        let open = self.delimiter();
                        let body = self.list_until_right();
                        let close = self.delimiter();
                        Node::Fenced(open, Box::new(Node::Group(body)), close)
                    }
                    "{" => Node::Sym("{".into(), Class::Punct),
                    "}" => Node::Sym("}".into(), Class::Punct),
                    "%" | "$" | "#" | "&" | "_" => Node::Sym(name, Class::Plain),
                    n => match (big_op(n), symbol(n)) {
                        (Some(op), _) => Node::Big(op.into()),
                        (None, Some((s, cl))) => Node::Sym(s.into(), cl),
                        _ => Node::Text(format!("\\{n}")),
                    },
                }
            }
            c if c.is_ascii_digit() || c == '.' => {
                let mut s = c.to_string();
                while let Some(d) = self.peek().filter(|d| d.is_ascii_digit() || *d == '.') {
                    s.push(d);
                    self.i += 1;
                }
                Node::Sym(s, Class::Number)
            }
            c if c.is_alphabetic() => Node::Sym(c.to_string(), Class::Ident),
            '+' => Node::Sym("+".into(), Class::Bin),
            '-' => Node::Sym("−".into(), Class::Bin),
            '*' => Node::Sym("∗".into(), Class::Bin),
            '=' | '<' | '>' => Node::Sym(c.to_string(), Class::Rel),
            ',' | ';' => Node::Sym(c.to_string(), Class::Punct),
            '\'' => Node::Sym("′".into(), Class::Plain),
            c => Node::Sym(c.to_string(), Class::Plain),
        })
    }
    fn list(&mut self, end: Option<char>) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => break,
                Some(c) if Some(c) == end => {
                    self.i += 1;
                    break;
                }
                Some('^') | Some('_') => {
                    let up = self.peek() == Some('^');
                    self.i += 1;
                    let a = self.arg();
                    let base = out.pop().unwrap_or(Node::Group(vec![]));
                    let node = match base {
                        Node::Scripts { base, sup, sub } => {
                            if up {
                                Node::Scripts { base, sup: Some(Box::new(a)), sub }
                            } else {
                                Node::Scripts { base, sup, sub: Some(Box::new(a)) }
                            }
                        }
                        b => Node::Scripts { base: Box::new(b), sup: up.then(|| Box::new(a.clone())), sub: (!up).then(|| Box::new(a)) },
                    };
                    out.push(node);
                }
                Some(_) => {
                    if let Some(a) = self.atom() {
                        out.push(a);
                    }
                }
            }
        }
        out
    }
    fn list_until_right(&mut self) -> Vec<Node> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.s[self.i..].starts_with(&['\\', 'r', 'i', 'g', 'h', 't']) {
                self.i += 6;
                break;
            }
            let mut p = Parser { s: self.s, i: self.i };
            let one = p.list_one();
            self.i = p.i;
            out.extend(one);
        }
        out
    }
    /// One atom plus any scripts on it.
    fn list_one(&mut self) -> Vec<Node> {
        let mut out = Vec::new();
        if let Some(a) = self.atom() {
            out.push(a);
        }
        loop {
            self.skip_ws();
            match self.peek() {
                Some('^') | Some('_') => {
                    let up = self.peek() == Some('^');
                    self.i += 1;
                    let a = self.arg();
                    let base = out.pop().unwrap_or(Node::Group(vec![]));
                    out.push(match base {
                        Node::Scripts { base, sub, .. } if up => Node::Scripts { base, sup: Some(Box::new(a)), sub },
                        Node::Scripts { base, sup, .. } => Node::Scripts { base, sup, sub: Some(Box::new(a)) },
                        b => Node::Scripts { base: Box::new(b), sup: up.then(|| Box::new(a.clone())), sub: (!up).then(|| Box::new(a)) },
                    });
                }
                _ => break,
            }
        }
        out
    }
}

pub(super) fn parse(src: &str) -> Node {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { s: &chars, i: 0 };
    Node::Group(p.list(None))
}
