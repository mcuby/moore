// Automatically generated by pargen

fn action_s0(ctx: &mut Context, p: &mut impl AbstractParser) -> ReportedResult<()> {
    let t = p.peek(0);
    match t.0 {
        Token::Semicolon
        | Token::Keyword(Kw::Extern)
        | Token::Keyword(Kw::Module)
        | Token::Keyword(Kw::Macromodule)
        | Token::Keyword(Kw::Interface)
        | Token::Keyword(Kw::Program)
        | Token::Keyword(Kw::Checker)
        | Token::Keyword(Kw::Virtual)
        | Token::Keyword(Kw::Class)
        | Token::Keyword(Kw::Package)
        | Token::Keyword(Kw::Type)
        | Token::Keyword(Kw::Bind)
        | Token::Keyword(Kw::Const)
        | Token::Keyword(Kw::Function)
        | Token::Keyword(Kw::Static)
        | Token::Keyword(Kw::Constraint)
        | Token::OpenDelim(DelimToken::Brack)
        | Token::Keyword(Kw::Localparam)
        | Token::Keyword(Kw::Parameter)
        | Token::Keyword(Kw::Var)
        | Token::Keyword(Kw::Import)
        | Token::Keyword(Kw::Export)
        | Token::Keyword(Kw::Typedef)
        | Token::Keyword(Kw::Enum)
        | Token::Keyword(Kw::Struct)
        | Token::Keyword(Kw::Union)
        | Token::Keyword(Kw::Automatic)
        | Token::Keyword(Kw::String)
        | Token::Keyword(Kw::Chandle)
        | Token::Keyword(Kw::Event)
        | Token::Keyword(Kw::Bit)
        | Token::Keyword(Kw::Logic)
        | Token::Keyword(Kw::Reg)
        | Token::Keyword(Kw::Byte)
        | Token::Keyword(Kw::Shortint)
        | Token::Keyword(Kw::Int)
        | Token::Keyword(Kw::Longint)
        | Token::Keyword(Kw::Integer)
        | Token::Keyword(Kw::Time)
        | Token::Keyword(Kw::Shortreal)
        | Token::Keyword(Kw::Real)
        | Token::Keyword(Kw::Realtime)
        | Token::Keyword(Kw::Supply0)
        | Token::Keyword(Kw::Supply1)
        | Token::Keyword(Kw::Tri)
        | Token::Keyword(Kw::Triand)
        | Token::Keyword(Kw::Trior)
        | Token::Keyword(Kw::Trireg)
        | Token::Keyword(Kw::Tri0)
        | Token::Keyword(Kw::Tri1)
        | Token::Keyword(Kw::Uwire)
        | Token::Keyword(Kw::Wire)
        | Token::Keyword(Kw::Wand)
        | Token::Keyword(Kw::Wor)
        | Token::Keyword(Kw::Signed)
        | Token::Keyword(Kw::Unsigned)
        | Token::Keyword(Kw::Task)
        | Token::Keyword(Kw::Property)
        | Token::Keyword(Kw::Sequence)
        | Token::Keyword(Kw::Let)
        | Token::Ident(_)
        | Token::Eof => {
            // reduce n269 -> ε
            p.add_diag(DiagBuilder2::bug(format!("not yet supported: `{}`", t.0)).span(t.1));
            return Err(());
        }
        Token::Keyword(Kw::Timeunit) | Token::Keyword(Kw::Timeprecision) => {
            p.add_diag(
                DiagBuilder2::bug(format!(
                    "ambiguous: `{}` cannot be handled by the parser here",
                    t.0
                ))
                .span(t.1),
            );
            return Err(());
        }
        _ => {
            p.add_diag(
                DiagBuilder2::error(format!("syntax error: `{}` not possible here", t.0))
                    .span(t.1)
                    .add_note("expected source text")
                    .add_note("expected timeunits declaration"),
            );
            return Err(());
        }
    }
}
