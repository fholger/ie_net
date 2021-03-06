use anyhow::{anyhow, Result};

#[derive(PartialEq, Debug, Default)]
pub struct RawCommand {
    pub command: String,
    pub params: Vec<Vec<u8>>,
}

pub fn try_parse_raw_command(input: &[u8]) -> Result<RawCommand> {
    let (_, command) = parsers::client_command(input)
        .map_err(|_| anyhow!("Could not parse command from client data"))?;
    Ok(command)
}

mod parsers {
    use crate::messages::raw_command::RawCommand;
    use crate::util::bytevec_to_str;
    use nom::branch::alt;
    use nom::bytes::complete::{is_not, tag, take_till, take_while};
    use nom::character::complete::{char, multispace0, multispace1};
    use nom::character::is_alphabetic;
    use nom::combinator::opt;
    use nom::multi::separated_list;
    use nom::sequence::{delimited, preceded, tuple};
    use nom::IResult;

    fn command(input: &[u8]) -> IResult<&[u8], &[u8]> {
        preceded(char('/'), take_while(is_alphabetic))(input)
    }

    named!(end_of_input, eof!());

    fn quoted_param(input: &[u8]) -> IResult<&[u8], &[u8]> {
        delimited(
            char('"'),
            take_till(|c| c as char == '"'),
            alt((tag("\""), end_of_input)),
        )(input)
    }

    fn unquoted_param(input: &[u8]) -> IResult<&[u8], &[u8]> {
        is_not(" \t\"")(input)
    }

    fn any_param(input: &[u8]) -> IResult<&[u8], &[u8]> {
        alt((quoted_param, unquoted_param))(input)
    }

    fn param_list(input: &[u8]) -> IResult<&[u8], Vec<&[u8]>> {
        separated_list(multispace1, any_param)(input)
    }

    pub(super) fn client_command(input: &[u8]) -> IResult<&[u8], RawCommand> {
        let (input, command) = command(input)?;
        let (input, params) = opt(preceded(multispace1, param_list))(input)?;
        let (input, _) = tuple((multispace0, end_of_input))(input)?;
        Ok((
            input,
            RawCommand {
                command: bytevec_to_str(command).to_ascii_lowercase(),
                params: match params {
                    None => vec![],
                    Some(params) => params.iter().map(|x| x.to_vec()).collect(),
                },
            },
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use nom::error::ErrorKind;
        use nom::Err::Error;

        #[test]
        fn test_command() {
            assert_eq!(command(b"/hello"), Ok((&b""[..], &b"hello"[..])));
            assert_eq!(
                command(b"/WAT? is this"),
                Ok((&b"? is this"[..], &b"WAT"[..]))
            );
            assert_eq!(
                command(b"no command here"),
                Err(Error((&b"no command here"[..], ErrorKind::Char)))
            );
            assert_eq!(command(b"/?command"), Ok((&b"?command"[..], &b""[..])));
        }

        #[test]
        fn test_quoted_param() {
            assert_eq!(
                quoted_param(b"\"hello world! \" next"),
                Ok((&b" next"[..], &b"hello world! "[..]))
            );
            assert_eq!(
                quoted_param(b"\"missing end quote"),
                Ok((&b""[..], &b"missing end quote"[..]))
            );
            assert_eq!(
                quoted_param(b"\"hello \\ world\""),
                Ok((&b""[..], &b"hello \\ world"[..]))
            );
            assert_eq!(
                quoted_param(b"test"),
                Err(Error((&b"test"[..], ErrorKind::Char)))
            );
        }

        #[test]
        fn test_unquoted_param() {
            assert_eq!(
                unquoted_param(b"test! me"),
                Ok((&b" me"[..], &b"test!"[..]))
            );
            assert_eq!(
                unquoted_param(b"  test! me"),
                Err(Error((&b"  test! me"[..], ErrorKind::IsNot)))
            );
            assert_eq!(
                unquoted_param(b"\"test\""),
                Err(Error((&b"\"test\""[..], ErrorKind::IsNot)))
            );
        }

        #[test]
        fn test_param_list() {
            assert_eq!(
                param_list(b"a \"b \" c "),
                Ok((&b" "[..], vec![&b"a"[..], &b"b "[..], &b"c"[..]]))
            );
        }

        #[test]
        fn test_client_command_without_params() {
            assert_eq!(
                client_command(b"/noparams"),
                Ok((
                    &b""[..],
                    RawCommand {
                        command: "noparams".to_string(),
                        params: vec![],
                    }
                ))
            );
            assert_eq!(
                client_command(b"/withextraspace   "),
                Ok((
                    &b""[..],
                    RawCommand {
                        command: "withextraspace".to_string(),
                        params: vec![],
                    }
                ))
            );
            assert_eq!(
                client_command(b" /invalid"),
                Err(Error((&b" /invalid"[..], ErrorKind::Char)))
            );
        }

        #[test]
        fn test_client_command_with_params() {
            assert_eq!(
                client_command(
                    b"/cmd  param1 param2 \" a longer param\" param4 \"\" \"open ended  "
                ),
                Ok((
                    &b""[..],
                    RawCommand {
                        command: "cmd".to_string(),
                        params: vec![
                            b"param1".to_vec(),
                            b"param2".to_vec(),
                            b" a longer param".to_vec(),
                            b"param4".to_vec(),
                            b"".to_vec(),
                            b"open ended  ".to_vec()
                        ],
                    }
                ))
            );
        }
    }
}
