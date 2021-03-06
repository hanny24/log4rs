use std::borrow::ToOwned;
use std::collections::HashMap;
use time::Duration;

use toml_parser::Value;
use toml_parser as toml;
use log::LogLevelFilter;

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct Config {
    pub refresh_rate: Option<Duration>,
    pub root: Option<Root>,
    pub appenders: HashMap<String, Appender>,
    pub loggers: Vec<Logger>,
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct Root {
    pub level: LogLevelFilter,
    pub appenders: Option<Vec<String>>,
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct Appender {
    pub kind: String,
    pub filters: Option<Vec<Filter>>,
    pub config: toml::Table,
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct Filter {
    pub kind: String,
    pub config: toml::Table,
}

#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct Logger {
    pub name: String,
    pub level: LogLevelFilter,
    pub appenders: Option<Vec<String>>,
    pub additive: Option<bool>,
}

pub fn parse(config: &str) -> Result<Config, Vec<String>> {
    let mut parser = toml::Parser::new(config);
    match parser.parse() {
        Some(table) => {
            if parser.errors.is_empty() {
                finish_parse_config(table)
            } else {
                Err(make_errors(&parser))
            }
        }
        _ => Err(make_errors(&parser)),
    }
}

fn parse_level(level: toml::Value) -> Result<LogLevelFilter, Vec<String>> {
    match level {
        Value::String(level) => {
            if let Ok(level) = level.parse() {
                Ok(level)
            } else {
                Err(vec![format!("Invalid `level` \"{}\"", level)])
            }
        }
        _ => Err(vec!["`level` must be a string".to_owned()]),
    }
}

fn parse_appenders(appenders: toml::Value) -> Result<Vec<String>, Vec<String>> {
    match appenders {
        Value::Array(array) => {
            let mut appenders = vec![];
            for appender in array {
                match appender {
                    Value::String(appender) => appenders.push(appender),
                    _ => return Err(vec!["`appenders must be an array of strings".to_owned()]),
                }
            }
            Ok(appenders)
        }
        _ => Err(vec!["`appenders` must be an array of strings".to_owned()]),
    }
}

fn parse_root(root: toml::Value) -> Result<Root, Vec<String>> {
    let mut root = match root {
        Value::Table(root) => root,
        _ => return Err(vec!["`root` must be a table".to_owned()]),
    };

    let mut errors = vec![];

    let level = match root.remove("level") {
        Some(level) => {
            match parse_level(level) {
                Ok(level) => level,
                Err(errs) => {
                    errors.extend(errs.into_iter());
                    LogLevelFilter::Off
                }
            }
        }
        None => {
            errors.push("`root` must contain a `level`".to_owned());
            LogLevelFilter::Off
        }
    };

    let appenders = match root.remove("appenders") {
        Some(appenders) => {
            match parse_appenders(appenders) {
                Ok(appenders) => Some(appenders),
                Err(errs) => {
                    errors.extend(errs.into_iter());
                    None
                }
            }
        }
        None => None,
    };

    for key in root.keys() {
        errors.push(format!("unrecognized `root` key: {}", key));
    }

    if errors.is_empty() {
        Ok(Root {
            level: level,
            appenders: appenders,
        })
    } else {
        Err(errors)
    }
}

fn parse_filters(appender: &str, filters: toml::Value) -> Result<Vec<Filter>, Vec<String>> {
    match filters {
        Value::Array(filters) => {
            let mut errors = vec![];

            let filters = filters.into_iter()
                                 .filter_map(|filter| {
                                     match filter {
                                         Value::Table(mut filter) => {
                                             let kind = match filter.remove("kind") {
                                                 Some(Value::String(kind)) => kind,
                                                 Some(_) => {
                                                     errors.push(format!("`kind` must be a \
                                                                          string in filter for \
                                                                          appender {}",
                                                                         appender));
                                                     return None;
                                                 }
                                                 None => {
                                                     errors.push(format!("`kind` must be \
                                                                          present in filter for \
                                                                          appender {}",
                                                                         appender));
                                                     return None;
                                                 }
                                             };

                                             Some(Filter {
                                                 kind: kind,
                                                 config: filter,
                                             })
                                         }
                                         _ => {
                                             errors.push(format!("filter must be a table in \
                                                                  appender {}",
                                                                 appender));
                                             None
                                         }
                                     }
                                 })
                                 .collect();

            if errors.is_empty() {
                Ok(filters)
            } else {
                Err(errors)
            }
        }
        _ => Err(vec![format!("`filter` must be an array in appender {}", appender)]),
    }
}

fn finish_parse_config(mut table: toml::Table) -> Result<Config, Vec<String>> {
    let mut errors = vec![];

    let refresh_rate = match table.remove("refresh_rate") {
        Some(Value::Integer(refresh_rate)) => Some(Duration::seconds(refresh_rate)),
        Some(_) => {
            errors.push("`refresh_rate` must be an integer".to_owned());
            None
        }
        None => None,
    };

    let root = match table.remove("root") {
        Some(root) => {
            match parse_root(root) {
                Ok(root) => Some(root),
                Err(errs) => {
                    errors.extend(errs.into_iter());
                    None
                }
            }
        }
        None => None,
    };

    let appenders = match table.remove("appender") {
        Some(Value::Table(table)) => {
            table.into_iter()
                 .filter_map(|(name, spec)| {
                     let mut spec = match spec {
                         Value::Table(spec) => spec,
                         _ => {
                             errors.push(format!("{} should be a table", name));
                             return None;
                         }
                     };

                     let kind = match spec.remove("kind") {
                         Some(Value::String(kind)) => kind,
                         Some(_) => {
                             errors.push(format!("`kind` must be a string in appender {}", name));
                             return None;
                         }
                         None => {
                             errors.push(format!("`kind` must be present in appender {}", name));
                             return None;
                         }
                     };

                     let filters = match spec.remove("filter") {
                         Some(filters) => {
                             match parse_filters(&name, filters) {
                                 Ok(filters) => Some(filters),
                                 Err(errs) => {
                                     errors.extend(errs);
                                     None
                                 }
                             }
                         }
                         None => None,
                     };

                     let spec = Appender {
                         kind: kind,
                         config: spec,
                         filters: filters,
                     };

                     Some((name, spec))
                 })
                 .collect()
        }
        None => HashMap::new(),
        _ => {
            errors.push("`appender` should be a table".to_owned());
            HashMap::new()
        }
    };

    let loggers = match table.remove("logger") {
        Some(Value::Array(array)) => {
            array.into_iter()
                 .filter_map(|directive| {
                     if let Value::Table(mut table) = directive {
                         let name = match table.remove("name") {
                             Some(Value::String(name)) => name,
                             None => String::new(),
                             _ => {
                                 errors.push("`name` should be a string".to_owned());
                                 String::new()
                             }
                         };

                         let level = match table.remove("level") {
                             Some(level) => {
                                 match parse_level(level) {
                                     Ok(level) => level,
                                     Err(errs) => {
                                         errors.extend(errs.into_iter());
                                         LogLevelFilter::Off
                                     }
                                 }
                             }
                             None => {
                                 errors.push("`level` must be present in all `logger`s".to_owned());
                                 LogLevelFilter::Off
                             }
                         };

                         let appenders = match table.remove("appenders") {
                             Some(appenders) => {
                                 match parse_appenders(appenders) {
                                     Ok(appenders) => Some(appenders),
                                     Err(errs) => {
                                         errors.extend(errs.into_iter());
                                         None
                                     }
                                 }
                             }
                             None => None,
                         };

                         let additive = match table.remove("additive") {
                             Some(Value::Boolean(additive)) => Some(additive),
                             Some(_) => {
                                 errors.push("`additive` must be a boolean".to_owned());
                                 None
                             }
                             None => None,
                         };

                         for key in table.keys() {
                             errors.push(format!("unrecognized `logger` key: {}", key));
                         }

                         Some(Logger {
                             name: name,
                             level: level,
                             appenders: appenders,
                             additive: additive,
                         })
                     } else {
                         errors.push("`logger` should contain tables".to_owned());
                         None
                     }
                 })
                 .collect()
        }
        None => vec![],
        _ => {
            errors.push("`logger` should be an array".to_owned());
            vec![]
        }
    };

    if errors.is_empty() {
        Ok(Config {
            refresh_rate: refresh_rate,
            appenders: appenders,
            root: root,
            loggers: loggers,
        })
    } else {
        Err(errors)
    }
}

fn make_errors(parser: &toml::Parser) -> Vec<String> {
    parser.errors
          .iter()
          .map(|error| {
              let (lo_line, lo_col) = parser.to_linecol(error.lo);
              let (hi_line, hi_col) = parser.to_linecol(error.hi);
              format!("{}:{}: {}:{} {}",
                      lo_line,
                      lo_col,
                      hi_line,
                      hi_col,
                      error.desc)
          })
          .collect()
}

#[cfg(test)]
mod test {
    use std::borrow::ToOwned;
    use std::collections::{HashMap, BTreeMap};
    use time::Duration;
    use toml_parser::Value;
    use log::LogLevelFilter;

    use super::*;

    #[test]
    fn test_basic() {
        let cfg = r#"
refresh_rate = 60

[appender.console]
kind = "console"

[[appender.console.filter]]
kind = "threshold"
level = "debug"

[appender.baz]
kind = "file"
file = "log/baz.log"

[root]
appenders = ["console"]
level = "info"

[[logger]]
name = "foo::bar::baz"
level = "warn"
appenders = ["baz"]
additive = false
"#;

        let actual = parse(cfg).unwrap();

        let expected = Config {
            refresh_rate: Some(Duration::seconds(60)),
            appenders: {
                let mut m = HashMap::new();
                m.insert("console".to_owned(),
                         Appender {
                             kind: "console".to_owned(),
                             config: BTreeMap::new(),
                             filters: Some(vec![Filter {
                                                    kind: "threshold".to_string(),
                                                    config: {
                                                        let mut m = BTreeMap::new();
                                                        m.insert("level".to_string(),
                                                                 Value::String("debug"
                                                                                   .to_string()));
                                                        m
                                                    },
                                                }]),
                         });
                m.insert("baz".to_owned(),
                         Appender {
                             kind: "file".to_owned(),
                             config: {
                                 let mut m = BTreeMap::new();
                                 m.insert("file".to_owned(),
                                          Value::String("log/baz.log".to_owned()));
                                 m
                             },
                             filters: None,
                         });
                m
            },
            root: Some(Root {
                level: LogLevelFilter::Info,
                appenders: Some(vec!["console".to_owned()]),
            }),
            loggers: vec![
                Logger {
                    name: "foo::bar::baz".to_owned(),
                    level: LogLevelFilter::Warn,
                    appenders: Some(vec!["baz".to_owned()]),
                    additive: Some(false)
                },
            ],
        };

        assert_eq!(expected, actual);
    }
}
