//! LaTeX math -> a small math AST (recursive-descent, char-based).
//!
//! Covers (this milestone): runs, groups `{…}`, `\frac`/`\dfrac`/`\tfrac`,
//! `^`/`_` scripts (either order), `\sqrt` and `\sqrt[n]`, symbol commands
//! (Greek/operators via [`crate::symbols`]) and upright function names.

use crate::symbols;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A literal run of (math-italic by default) text.
    Run(String),
    /// An upright run (function names, `\mathrm`-like).
    Upright(String),
    /// A sequence of nodes.
    Row(Vec<Node>),
    /// A fraction: numerator over denominator.
    Frac(Box<Node>, Box<Node>),
    /// A stacked fraction without a line or implicit parentheses.
    NoBarFrac(Box<Node>, Box<Node>),
    Sup(Box<Node>, Box<Node>),
    Sub(Box<Node>, Box<Node>),
    SubSup(Box<Node>, Box<Node>, Box<Node>),
    /// A square root.
    Sqrt(Box<Node>),
    /// An nth root: (index, radicand).
    Root(Box<Node>, Box<Node>),
    /// A big/n-ary operator (∑/∫/∏/…) with optional lower/upper limits and an
    /// operand. `over_under` places the limits above/below (sums) vs. as scripts
    /// (integrals).
    Nary {
        op: String,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
        body: Box<Node>,
        over_under: bool,
    },
    /// A `\left<d> … \right<d>` delimited group (empty delim = `\left.`).
    Delim {
        open: String,
        close: String,
        body: Box<Node>,
    },
    /// A math accent (`\hat`/`\vec`/`\tilde`/…): a combining char over the base.
    Accent {
        chr: char,
        base: Box<Node>,
    },
    /// An over/under bar (`\overline`/`\underline`).
    Bar {
        top: bool,
        base: Box<Node>,
    },
    /// A matrix / `aligned` / `cases` grid (rows of cells), optionally wrapped in
    /// delimiters (`pmatrix` → `()`, `bmatrix` → `[]`, `cases` → `{`).
    Matrix {
        rows: Vec<Vec<Node>>,
        delim: Option<(String, String)>,
    },
    /// A styled run: an OMML `m:sty` value (`b`/`i`/`bi`/`p`) over literal text.
    /// `\mathbf` → `b`; `\mathit` → `i`.
    Styled {
        sty: &'static str,
        text: String,
    },
    /// A binomial coefficient (`\binom{n}{k}`): parentheses over a bar-less
    /// fraction.
    Binom(Box<Node>, Box<Node>),
}

/// Maximum nesting depth the parser will descend before bailing out. Deeply
/// nested input (e.g. thousands of nested `\frac`/`{`/`\left`) would otherwise
/// overflow the stack; past this depth the remaining content is dropped.
const MAX_DEPTH: usize = 64;

/// Parse a LaTeX math string into a [`Node::Row`].
pub fn parse(latex: &str) -> Node {
    parse_with_depth(latex, 0).0
}

pub fn parse_checked(latex: &str) -> Result<Node, String> {
    let (node, error) = parse_with_depth(latex, 0);
    match error {
        Some(error) => Err(error),
        None => Ok(node),
    }
}

/// Parse a sub-string seeded with the caller's current nesting depth, so that
/// recursion crossing a fresh sub-parser (`\left…\right`, matrix cells) still
/// counts toward [`MAX_DEPTH`] and cannot overflow the stack.
fn parse_with_depth(latex: &str, depth: usize) -> (Node, Option<String>) {
    let mut p = Parser {
        s: latex.chars().collect(),
        i: 0,
        depth,
        error: None,
    };
    let node = p.parse_row();
    if p.i != p.s.len() {
        p.fail("存在多余的右花括号");
    }
    (node, p.error)
}

struct Parser {
    s: Vec<char>,
    i: usize,
    depth: usize,
    error: Option<String>,
}

impl Parser {
    fn fail(&mut self, message: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(message.into());
        }
    }

    fn fragment(&mut self, source: &str) -> Node {
        let (node, error) = parse_with_depth(source, self.depth);
        if let Some(error) = error {
            self.fail(error);
        }
        node
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\n' | '\t' | '\r')) {
            self.i += 1;
        }
    }

    fn parse_row(&mut self) -> Node {
        let mut items: Vec<Node> = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                None | Some('}') => break,
                _ => {}
            }
            let atom = self.parse_atom();
            let atom = self.maybe_scripts(atom);
            push_merge(&mut items, atom);
        }
        Node::Row(items)
    }

    /// A single atom: a `{…}` group, a command, or one character.
    fn parse_atom(&mut self) -> Node {
        self.skip_space();
        // Guard against stack overflow on pathologically nested input. Once the
        // limit is hit, consume the rest of the current group as a plain run so
        // the scan still terminates.
        if self.depth >= MAX_DEPTH {
            self.fail("公式嵌套超过 64 层");
            let mut text = String::new();
            while let Some(c) = self.peek() {
                if c == '}' {
                    break;
                }
                text.push(c);
                self.i += 1;
            }
            return Node::Run(text);
        }
        self.depth += 1;
        let node = match self.peek() {
            Some('{') => {
                self.i += 1;
                let r = self.parse_row();
                if self.peek() == Some('}') {
                    self.i += 1;
                } else {
                    self.fail("花括号未闭合");
                }
                r
            }
            Some('\\') => self.parse_command(),
            Some('}' | '^' | '_' | '&' | '%' | '#' | '$') => {
                self.fail("公式缺少参数或包含未转义的特殊字符");
                self.i += 1;
                Node::Run(String::new())
            }
            Some(c) => {
                self.i += 1;
                Node::Run(c.to_string())
            }
            None => {
                self.fail("公式命令缺少参数");
                Node::Run(String::new())
            }
        };
        self.depth -= 1;
        node
    }

    /// After an atom, attach any `^`/`_` scripts (in either order).
    fn maybe_scripts(&mut self, base: Node) -> Node {
        let mut sub: Option<Box<Node>> = None;
        let mut sup: Option<Box<Node>> = None;
        loop {
            self.skip_space();
            match self.peek() {
                Some('^') => {
                    if sup.is_some() {
                        self.fail("同一项出现重复上标");
                    }
                    self.i += 1;
                    self.skip_space();
                    sup = Some(Box::new(self.parse_atom()));
                }
                Some('_') => {
                    if sub.is_some() {
                        self.fail("同一项出现重复下标");
                    }
                    self.i += 1;
                    self.skip_space();
                    sub = Some(Box::new(self.parse_atom()));
                }
                _ => break,
            }
        }
        match (sub, sup) {
            (None, None) => base,
            (Some(sb), None) => Node::Sub(Box::new(base), sb),
            (None, Some(sp)) => Node::Sup(Box::new(base), sp),
            (Some(sb), Some(sp)) => Node::SubSup(Box::new(base), sb, sp),
        }
    }

    /// Read a command name (letters), or a single non-letter control symbol.
    fn read_name(&mut self) -> String {
        self.i += 1; // consume '\'
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                let start = self.i;
                while matches!(self.peek(), Some(c) if c.is_ascii_alphabetic()) {
                    self.i += 1;
                }
                self.s[start..self.i].iter().collect()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    fn parse_command(&mut self) -> Node {
        let name = self.read_name();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                self.skip_space();
                if name == "cfrac" && self.peek() == Some('[') {
                    self.fail(r"暂不支持 \cfrac 的分子对齐参数");
                }
                let num = self.parse_atom();
                let den = self.parse_atom();
                Node::Frac(Box::new(num), Box::new(den))
            }
            "genfrac" => self.parse_genfrac(),
            "sqrt" => {
                self.skip_space();
                if self.peek() == Some('[') {
                    let idx = self.read_bracket();
                    let rad = self.parse_atom();
                    Node::Root(Box::new(idx), Box::new(rad))
                } else {
                    Node::Sqrt(Box::new(self.parse_atom()))
                }
            }
            "text" | "operatorname" => Node::Upright(self.read_text()),
            "mathrm" | "mathsf" | "mathtt" => {
                // upright content (font nuances beyond upright are not modelled)
                Node::Upright(self.styled_text())
            }
            "mathbf" | "boldsymbol" | "bm" => Node::Styled {
                sty: "b",
                text: self.styled_text(),
            },
            "mathit" => Node::Run(self.styled_text()), // italic = default
            // math-class wrappers only affect spacing -> render content transparently
            "mathbin" | "mathrel" | "mathop" | "mathord" | "mathopen" | "mathclose"
            | "mathpunct" | "mathinner" => self.parse_atom(),
            // script/blackboard/fraktur alphabets -> Unicode math-alphanumerics
            "mathbb" | "mathcal" | "mathscr" | "mathfrak" => {
                Node::Run(symbols::alphabet(&name, &self.styled_text()))
            }
            "binom" | "dbinom" | "tbinom" => {
                let n = self.parse_atom();
                let k = self.parse_atom();
                Node::Binom(Box::new(n), Box::new(k))
            }
            "pmod" => Node::Row(vec![
                Node::Run(" (".into()),
                Node::Upright("mod".into()),
                Node::Run(" ".into()),
                self.parse_atom(),
                Node::Run(")".into()),
            ]),
            "bmod" => Node::Upright("mod".into()),
            "left" => self.parse_delim(),
            "right" => {
                self.fail(r"\right 缺少对应的 \left");
                Node::Run(String::new())
            }
            "begin" => self.parse_environment(),
            "end" => {
                self.fail(r"\end 缺少对应的 \begin，或环境名称不匹配");
                let _ = self.read_raw_group(); // stray \end{env}: consume its name
                Node::Run(String::new())
            }
            "overline" => Node::Bar {
                top: true,
                base: Box::new(self.parse_atom()),
            },
            "underline" => Node::Bar {
                top: false,
                base: Box::new(self.parse_atom()),
            },
            _ => {
                if let Some(chr) = math_accent(&name) {
                    Node::Accent {
                        chr,
                        base: Box::new(self.parse_atom()),
                    }
                } else if let Some((op, over_under)) = symbols::nary(&name) {
                    let (sub, sup) = self.parse_nary_limits();
                    let body = self.parse_operand();
                    Node::Nary {
                        op: op.to_string(),
                        sub,
                        sup,
                        body: Box::new(body),
                        over_under,
                    }
                } else if let Some(f) = symbols::function_name(&name) {
                    Node::Upright(f.to_string())
                } else if let Some(sym) = symbols::symbol(&name) {
                    Node::Run(sym.to_string())
                } else {
                    let display: String = name.chars().take(80).collect();
                    self.fail(format!("暂不支持公式命令 \\{display}"));
                    Node::Run(String::new())
                }
            }
        }
    }

    fn styled_text(&mut self) -> String {
        let node = self.parse_atom();
        match flatten_text(&node) {
            Some(text) => text,
            None => {
                self.fail("暂不支持字体命令内的分式、上下标等复合结构");
                String::new()
            }
        }
    }

    fn read_text(&mut self) -> String {
        let source = self.read_raw_group();
        let mut chars = source.chars();
        let mut text = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some(c @ ('{' | '}' | '%' | '#' | '&' | '_' | '$' | ' ')) => text.push(c),
                    _ => self.fail("暂不支持文本命令内的嵌套命令"),
                },
                '{' | '}' => {} // plain TeX grouping
                '$' => self.fail("暂不支持文本命令内嵌数学公式"),
                _ => text.push(c),
            }
        }
        text
    }

    fn parse_genfrac(&mut self) -> Node {
        let open = self.read_raw_group();
        let close = self.read_raw_group();
        let thickness = self.read_raw_group();
        let style = self.read_raw_group();
        let num = Box::new(self.parse_atom());
        let den = Box::new(self.parse_atom());
        // The importer uses exactly the zero-thickness form. A custom line
        // thickness or forced style cannot be represented faithfully here.
        if !style.trim().is_empty() {
            self.fail(r"暂不支持 \genfrac 的指定字号参数");
        }
        let fraction = match thickness.trim() {
            "0" | "0pt" => Node::NoBarFrac(num, den),
            "" => Node::Frac(num, den),
            _ => {
                self.fail(r"暂不支持 \genfrac 的自定义线宽");
                Node::Frac(num, den)
            }
        };
        let open = self.genfrac_delimiter(&open);
        let close = self.genfrac_delimiter(&close);
        if open.is_empty() && close.is_empty() {
            fraction
        } else {
            Node::Delim {
                open,
                close,
                body: Box::new(fraction),
            }
        }
    }

    fn genfrac_delimiter(&mut self, source: &str) -> String {
        let source = source.trim();
        match source {
            "" | "." => String::new(),
            "(" | ")" | "[" | "]" | "|" | "/" => source.into(),
            _ if source.starts_with('\\') && !delim_symbol(&source[1..]).is_empty() => {
                delim_symbol(&source[1..]).into()
            }
            _ => {
                self.fail(r"\genfrac 包含不支持的定界符");
                String::new()
            }
        }
    }

    /// Read an n-ary operator's `_lower`/`^upper` limits (in either order).
    fn parse_nary_limits(&mut self) -> (Option<Box<Node>>, Option<Box<Node>>) {
        let mut sub = None;
        let mut sup = None;
        loop {
            self.skip_space();
            match self.peek() {
                Some('_') => {
                    if sub.is_some() {
                        self.fail("运算符出现重复下限");
                    }
                    self.i += 1;
                    self.skip_space();
                    sub = Some(Box::new(self.parse_atom()));
                }
                Some('^') => {
                    if sup.is_some() {
                        self.fail("运算符出现重复上限");
                    }
                    self.i += 1;
                    self.skip_space();
                    sup = Some(Box::new(self.parse_atom()));
                }
                _ => break,
            }
        }
        (sub, sup)
    }

    /// The operand of an n-ary operator: the next atom (with its own scripts).
    /// Kept to a single atom so a trailing `+ c` / `=` is not swallowed.
    fn parse_operand(&mut self) -> Node {
        self.skip_space();
        match self.peek() {
            None | Some('}') => Node::Row(Vec::new()),
            _ => {
                let a = self.parse_atom();
                self.maybe_scripts(a)
            }
        }
    }

    /// True if a complete `\name` control word begins at the cursor.
    fn at_command(&self, name: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let mut k = self.i + 1;
        for pc in name.chars() {
            if self.s.get(k) != Some(&pc) {
                return false;
            }
            k += 1;
        }
        !matches!(self.s.get(k), Some(c) if c.is_ascii_alphabetic())
    }

    /// Parse `\left<open> … \right<close>` (the `\left` is already consumed).
    fn parse_delim(&mut self) -> Node {
        let open = self.read_delim();
        let start = self.i;
        let mut depth = 1;
        while self.i < self.s.len() {
            if self.at_command("left") {
                depth += 1;
                self.i += 5; // "\left"
                continue;
            }
            if self.at_command("right") {
                depth -= 1;
                if depth == 0 {
                    let inner: String = self.s[start..self.i].iter().collect();
                    self.i += 6; // "\right"
                    let close = self.read_delim();
                    return Node::Delim {
                        open,
                        close,
                        body: Box::new(self.fragment(&inner)),
                    };
                }
                self.i += 6;
                continue;
            }
            self.i += 1;
        }
        // no matching \right: take the rest as the body
        self.fail(r"\left 缺少对应的 \right");
        let inner: String = self.s[start..].iter().collect();
        self.i = self.s.len();
        Node::Delim {
            open,
            close: String::new(),
            body: Box::new(self.fragment(&inner)),
        }
    }

    /// Read a balanced `{ … }` group's inner text and advance past it.
    fn read_raw_group(&mut self) -> String {
        self.skip_space();
        if self.peek() != Some('{') {
            self.fail("公式命令缺少花括号参数");
            return String::new();
        }
        self.i += 1;
        let start = self.i;
        let mut depth = 1;
        while self.i < self.s.len() {
            match self.s[self.i] {
                '\\' => {
                    // Skip the escaped char, but never advance past the end (a
                    // trailing `\` must not push the cursor beyond `len`).
                    self.i = (self.i + 2).min(self.s.len());
                    continue;
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner: String = self.s[start..self.i].iter().collect();
                        self.i += 1;
                        return inner;
                    }
                }
                _ => {}
            }
            self.i += 1;
        }
        self.fail("公式参数的花括号未闭合");
        self.s[start..].iter().collect()
    }

    fn matches(&self, pat: &[char]) -> bool {
        self.i + pat.len() <= self.s.len() && self.s[self.i..self.i + pat.len()] == *pat
    }

    /// Parse `\begin{env} … \end{env}` (the `\begin` is already consumed).
    fn parse_environment(&mut self) -> Node {
        let raw = self.read_raw_group();
        let env = raw.trim().trim_end_matches('*').to_string();
        if !matches!(
            env.as_str(),
            "matrix"
                | "pmatrix"
                | "bmatrix"
                | "Bmatrix"
                | "vmatrix"
                | "Vmatrix"
                | "smallmatrix"
                | "cases"
                | "aligned"
                | "align"
                | "gathered"
                | "gather"
                | "array"
                | "alignat"
        ) {
            self.fail(format!("暂不支持公式环境 {raw}"));
        }
        // Column specifications with rules/alignment cannot be represented by
        // this renderer. Report them instead of silently discarding them.
        if env == "array" || env == "alignat" {
            self.skip_space();
            if self.peek() == Some('{') {
                let columns = self.read_raw_group();
                if env == "array" && !columns.chars().all(|c| c == 'c' || c.is_whitespace()) {
                    self.fail("暂不支持数组的非居中列或列分隔线");
                }
            } else {
                self.fail("公式环境缺少列参数");
            }
        }
        let body = self.read_env_body(raw.trim());
        let rows = split_matrix(&body)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| self.fragment(cell.trim()))
                    .collect()
            })
            .collect();
        Node::Matrix {
            rows,
            delim: matrix_delim(&env),
        }
    }

    /// Read the body up to the matching `\end{env}` (nesting-aware).
    fn read_env_body(&mut self, env: &str) -> String {
        let bpat: Vec<char> = format!("\\begin{{{env}}}").chars().collect();
        let epat: Vec<char> = format!("\\end{{{env}}}").chars().collect();
        let start = self.i;
        let mut depth = 1;
        while self.i < self.s.len() {
            if self.matches(&bpat) {
                depth += 1;
                self.i += bpat.len();
                continue;
            }
            if self.matches(&epat) {
                depth -= 1;
                if depth == 0 {
                    let inner: String = self.s[start..self.i].iter().collect();
                    self.i += epat.len();
                    return inner;
                }
                self.i += epat.len();
                continue;
            }
            self.i += 1;
        }
        let inner: String = self.s[start..].iter().collect();
        self.i = self.s.len();
        self.fail(format!("公式环境 {env} 未闭合"));
        inner
    }

    /// Read a delimiter after `\left`/`\right`: a char, a `\cmd`, or `.` (none).
    fn read_delim(&mut self) -> String {
        self.skip_space();
        match self.peek() {
            Some('\\') => {
                let name = self.read_name();
                if delim_symbol(&name).is_empty() {
                    self.fail(format!("暂不支持定界符 \\{name}"));
                }
                delim_symbol(&name).to_string()
            }
            Some('.') => {
                self.i += 1;
                String::new()
            }
            Some(c) => {
                if !matches!(
                    c,
                    '(' | ')' | '[' | ']' | '|' | '/' | '⟨' | '⟩' | '⌈' | '⌉' | '⌊' | '⌋' | '‖'
                ) {
                    self.fail("公式包含无效的定界符");
                }
                self.i += 1;
                c.to_string()
            }
            None => {
                self.fail("公式缺少定界符");
                String::new()
            }
        }
    }

    /// Read a `[ … ]` optional group and parse its content as math.
    fn read_bracket(&mut self) -> Node {
        self.i += 1; // consume '['
        let start = self.i;
        while self.peek().is_some() && self.peek() != Some(']') {
            self.i += 1;
        }
        let inner: String = self.s[start..self.i].iter().collect();
        if self.peek() == Some(']') {
            self.i += 1;
        } else {
            self.fail("根式的方括号未闭合");
        }
        self.fragment(&inner)
    }
}

/// Drop empty runs and merge adjacent plain runs into one.
fn push_merge(items: &mut Vec<Node>, atom: Node) {
    if let Node::Run(s) = &atom {
        if s.is_empty() {
            return;
        }
        if let Some(Node::Run(prev)) = items.last_mut() {
            prev.push_str(s);
            return;
        }
    }
    items.push(atom);
}

/// Flatten a node to its plain text (for `\mathrm`/`\text` upright content).
fn flatten_text(node: &Node) -> Option<String> {
    match node {
        Node::Run(s) | Node::Upright(s) => Some(s.clone()),
        Node::Row(items) => items.iter().map(flatten_text).collect(),
        _ => None,
    }
}

/// Split a matrix/aligned body into rows (top-level `\\`) of cells (top-level
/// `&`), parsing each cell as math. Trailing empty rows (from a final `\\`) drop.
fn split_matrix(body: &str) -> Vec<Vec<String>> {
    let s: Vec<char> = body.chars().collect();
    let n = s.len();
    let mut rows: Vec<Vec<String>> = vec![vec![String::new()]];
    let mut depth = 0;
    let mut environment_depth = 0usize;
    let mut i = 0;
    while i < n {
        let c = s[i];
        if c == '\\' {
            if i + 1 < n && s[i + 1] == '\\' && depth == 0 && environment_depth == 0 {
                // row break; skip an optional [spacing] argument
                let mut j = i + 2;
                while j < n && matches!(s[j], ' ' | '\n' | '\t' | '\r') {
                    j += 1;
                }
                if j < n && s[j] == '[' {
                    while j < n && s[j] != ']' {
                        j += 1;
                    }
                    if j < n {
                        j += 1;
                    }
                }
                rows.push(vec![String::new()]);
                i = j;
                continue;
            }
            // a command: copy `\` + its name so an `&`/`\\` inside doesn't split
            let start = i;
            let mut j = i + 1;
            if j < n && s[j].is_ascii_alphabetic() {
                while j < n && s[j].is_ascii_alphabetic() {
                    j += 1;
                }
            } else if j < n {
                j += 1;
            }
            let cmd: String = s[start + 1..j].iter().collect();
            if cmd == "begin" {
                environment_depth += 1;
            } else if cmd == "end" {
                environment_depth = environment_depth.saturating_sub(1);
            }
            if cmd == "cr" && depth == 0 && environment_depth == 0 {
                // plain-TeX row separator, equivalent to `\\`
                rows.push(vec![String::new()]);
                i = j;
                continue;
            }
            rows.last_mut()
                .unwrap()
                .last_mut()
                .unwrap()
                .extend(&s[start..j]);
            i = j;
            continue;
        }
        match c {
            '{' => {
                depth += 1;
                rows.last_mut().unwrap().last_mut().unwrap().push(c);
            }
            '}' => {
                depth -= 1;
                rows.last_mut().unwrap().last_mut().unwrap().push(c);
            }
            '&' if depth == 0 && environment_depth == 0 => {
                rows.last_mut().unwrap().push(String::new())
            }
            _ => rows.last_mut().unwrap().last_mut().unwrap().push(c),
        }
        i += 1;
    }
    rows.into_iter()
        .filter(|row| row.iter().any(|cell| !cell.trim().is_empty()))
        .collect()
}

/// Delimiters that wrap a given matrix environment (`None` = bare `m:m`).
fn matrix_delim(env: &str) -> Option<(String, String)> {
    let (o, c) = match env {
        "pmatrix" => ("(", ")"),
        "bmatrix" => ("[", "]"),
        "Bmatrix" => ("{", "}"),
        "vmatrix" => ("|", "|"),
        "Vmatrix" => ("‖", "‖"),
        "cases" => ("{", ""),
        _ => return None, // matrix, smallmatrix, array, aligned, align, gathered…
    };
    Some((o.to_string(), c.to_string()))
}

/// Map a math accent command to its combining character (`m:acc` chr).
fn math_accent(name: &str) -> Option<char> {
    Some(match name {
        "hat" | "widehat" => '\u{0302}',
        "tilde" | "widetilde" => '\u{0303}',
        "bar" => '\u{0304}',
        "vec" | "overrightarrow" => '\u{20D7}',
        "dot" => '\u{0307}',
        "ddot" => '\u{0308}',
        "dddot" => '\u{20DB}',
        "check" => '\u{030C}',
        "breve" => '\u{0306}',
        "acute" => '\u{0301}',
        "grave" => '\u{0300}',
        "mathring" => '\u{030A}',
        _ => return None,
    })
}

/// Map a `\left`/`\right` delimiter command name to its glyph.
fn delim_symbol(name: &str) -> &'static str {
    match name {
        "{" | "lbrace" => "{",
        "}" | "rbrace" => "}",
        "|" | "Vert" | "lVert" | "rVert" => "‖",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "backslash" => "\\",
        "uparrow" => "↑",
        "downarrow" => "↓",
        other => symbols::symbol(other).unwrap_or(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_either_order() {
        assert_eq!(
            parse("x^2"),
            Node::Row(vec![Node::Sup(
                Box::new(Node::Run("x".into())),
                Box::new(Node::Run("2".into()))
            )])
        );
        // x_i^n and x^n_i both -> SubSup
        assert!(matches!(
            &parse("x_i^n"),
            Node::Row(v) if matches!(v[0], Node::SubSup(..))
        ));
        assert!(matches!(
            &parse("x^n_i"),
            Node::Row(v) if matches!(v[0], Node::SubSup(..))
        ));
    }

    #[test]
    fn frac_and_sqrt_and_root() {
        assert!(matches!(&parse(r"\frac{1}{2}"), Node::Row(v) if matches!(v[0], Node::Frac(..))));
        assert!(matches!(&parse(r"\sqrt{2}"), Node::Row(v) if matches!(v[0], Node::Sqrt(..))));
        assert!(matches!(&parse(r"\sqrt[3]{x}"), Node::Row(v) if matches!(v[0], Node::Root(..))));
    }

    #[test]
    fn symbols_and_functions() {
        assert_eq!(parse(r"\alpha"), Node::Row(vec![Node::Run("α".into())]));
        assert_eq!(parse(r"\sin"), Node::Row(vec![Node::Upright("sin".into())]));
    }

    #[test]
    fn adjacent_runs_merge() {
        assert_eq!(parse("abc"), Node::Row(vec![Node::Run("abc".into())]));
    }

    #[test]
    fn truncated_environment_does_not_panic() {
        // A trailing backslash inside `\begin{…}` must not slice out of bounds.
        let _ = parse(r"\begin{x\");
        let _ = parse(r"\begin{ab\");
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        // Pathologically nested input is bounded by MAX_DEPTH instead of
        // overflowing the stack.
        let _ = parse(&r"\frac{1}{".repeat(50_000));
        let _ = parse(&"{".repeat(100_000));
        let _ = parse(&r"\left(".repeat(50_000));
    }
}
