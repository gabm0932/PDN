//! Dockerfile parser for PDN image builds

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Instructions supported in PDN Dockerfile subset
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockerfileInstruction {
    From {
        image: String,
        tag: Option<String>,
        as_name: Option<String>,
    },
    Run(String),
    Copy {
        src: String,
        dest: String,
    },
    Add {
        src: String,
        dest: String,
    },
    Env {
        key: String,
        value: String,
    },
    Workdir(String),
    Expose(u16),
    Entrypoint(Vec<String>),
    Cmd(Vec<String>),
    Label {
        key: String,
        value: String,
    },
    User(String),
}

/// Strip enclosing single or double quotes
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parse FROM instruction argument
fn parse_from(rest: &str) -> Result<DockerfileInstruction> {
    let tokens: Vec<&str> = rest
        .split_whitespace()
        .filter(|t| !t.starts_with("--"))
        .collect();

    if tokens.is_empty() {
        bail!("FROM instruction requires an image name");
    }

    let (image_spec, as_name) =
        if let Some(as_pos) = tokens.iter().position(|t| t.eq_ignore_ascii_case("AS")) {
            let name = tokens.get(as_pos + 1).map(|s| s.to_string());
            let spec = tokens[..as_pos].join(" ");
            (spec, name)
        } else {
            (tokens.join(" "), None)
        };

    let (image, tag) = if let Some(last_slash) = image_spec.rfind('/') {
        let (repo, filename) = image_spec.split_at(last_slash + 1);
        if let Some((img_name, tag_str)) = filename.rsplit_once(':') {
            (
                format!("{}{}", repo, img_name),
                if tag_str.is_empty() {
                    None
                } else {
                    Some(tag_str.to_string())
                },
            )
        } else {
            (image_spec, None)
        }
    } else if let Some((img_name, tag_str)) = image_spec.rsplit_once(':') {
        (
            img_name.to_string(),
            if tag_str.is_empty() {
                None
            } else {
                Some(tag_str.to_string())
            },
        )
    } else {
        (image_spec, None)
    };

    Ok(DockerfileInstruction::From {
        image,
        tag,
        as_name,
    })
}

/// Parse COPY or ADD instruction arguments
fn parse_copy_or_add(rest: &str, is_add: bool) -> Result<DockerfileInstruction> {
    let trimmed = rest.trim();
    let (src, dest) = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let mut items: Vec<String> =
            serde_json::from_str(trimmed).context("Failed to parse JSON array in COPY/ADD")?;
        if items.len() < 2 {
            bail!("COPY/ADD requires at least 2 arguments");
        }
        let dest = items.pop().unwrap();
        let src = items.join(" ");
        (src, dest)
    } else {
        let mut parts: Vec<&str> = trimmed
            .split_whitespace()
            .filter(|p| !p.starts_with("--"))
            .collect();
        if parts.len() < 2 {
            bail!("COPY/ADD requires source and destination paths");
        }
        let dest = parts.pop().unwrap().to_string();
        let src = parts.join(" ");
        (src, dest)
    };

    if is_add {
        Ok(DockerfileInstruction::Add { src, dest })
    } else {
        Ok(DockerfileInstruction::Copy { src, dest })
    }
}

/// Parse key-value arguments for ENV and LABEL
fn parse_key_value(rest: &str) -> Result<(String, String)> {
    let trimmed = rest.trim();
    let (k, v) = if let Some((k, v)) = trimmed.split_once('=') {
        (k.trim(), v.trim())
    } else if let Some((k, v)) = trimmed.split_once(char::is_whitespace) {
        (k.trim(), v.trim())
    } else {
        bail!("Expected KEY=VALUE or KEY VALUE format: '{}'", rest);
    };

    let key = strip_quotes(k).to_string();
    let value = strip_quotes(v).to_string();
    Ok((key, value))
}

/// Parse EXPOSE instruction argument
fn parse_expose(rest: &str) -> Result<u16> {
    let trimmed = rest.trim();
    let port_str = trimmed.split('/').next().unwrap_or(trimmed).trim();
    let port = port_str
        .parse::<u16>()
        .with_context(|| format!("Invalid EXPOSE port '{}'", rest))?;
    Ok(port)
}

/// Parse ENTRYPOINT or CMD instruction arguments
fn parse_cmd_or_entrypoint(rest: &str) -> Result<Vec<String>> {
    let trimmed = rest.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        serde_json::from_str(trimmed).context("Failed to parse JSON array in ENTRYPOINT/CMD")
    } else {
        let words = trimmed
            .split_whitespace()
            .map(|s| strip_quotes(s).to_string())
            .collect();
        Ok(words)
    }
}

/// Parse a single logical instruction line
fn parse_instruction(line: &str) -> Result<DockerfileInstruction> {
    let (verb, rest) = match line.split_once(char::is_whitespace) {
        Some((v, r)) => (v.trim(), r.trim()),
        None => (line.trim(), ""),
    };

    match verb.to_ascii_uppercase().as_str() {
        "FROM" => parse_from(rest),
        "RUN" => Ok(DockerfileInstruction::Run(rest.to_string())),
        "COPY" => parse_copy_or_add(rest, false),
        "ADD" => parse_copy_or_add(rest, true),
        "ENV" => {
            let (key, value) = parse_key_value(rest)?;
            Ok(DockerfileInstruction::Env { key, value })
        }
        "WORKDIR" => Ok(DockerfileInstruction::Workdir(rest.to_string())),
        "EXPOSE" => {
            let port = parse_expose(rest)?;
            Ok(DockerfileInstruction::Expose(port))
        }
        "ENTRYPOINT" => {
            let args = parse_cmd_or_entrypoint(rest)?;
            Ok(DockerfileInstruction::Entrypoint(args))
        }
        "CMD" => {
            let args = parse_cmd_or_entrypoint(rest)?;
            Ok(DockerfileInstruction::Cmd(args))
        }
        "LABEL" => {
            let (key, value) = parse_key_value(rest)?;
            Ok(DockerfileInstruction::Label { key, value })
        }
        "USER" => Ok(DockerfileInstruction::User(rest.to_string())),
        unknown => bail!("Unsupported Dockerfile instruction: '{}'", unknown),
    }
}

/// Parse Dockerfile content into a list of instructions
pub fn parse_dockerfile(content: &str) -> Result<Vec<DockerfileInstruction>> {
    let mut instructions = Vec::new();
    let mut current_line = String::new();

    for line in content.lines() {
        let line = line.trim_end();
        let trimmed = line.trim();

        // Comments between lines or at top level
        if trimmed.starts_with('#') {
            continue;
        }

        if current_line.is_empty() && trimmed.is_empty() {
            continue;
        }

        if let Some(stripped) = line.strip_suffix('\\') {
            let stripped_part = stripped.trim_end();
            if current_line.is_empty() {
                current_line.push_str(stripped_part);
            } else if current_line.ends_with(|c: char| c.is_whitespace() || c == '=') {
                current_line.push_str(stripped_part.trim_start());
            } else {
                current_line.push(' ');
                current_line.push_str(stripped_part.trim_start());
            }
        } else {
            if current_line.is_empty() {
                current_line.push_str(trimmed);
            } else if current_line.ends_with(|c: char| c.is_whitespace() || c == '=') {
                current_line.push_str(trimmed);
            } else {
                current_line.push(' ');
                current_line.push_str(trimmed);
            }

            let full_stmt = std::mem::take(&mut current_line);
            let trimmed_stmt = full_stmt.trim();
            if !trimmed_stmt.is_empty() {
                instructions.push(parse_instruction(trimmed_stmt)?);
            }
        }
    }

    let final_stmt = current_line.trim();
    if !final_stmt.is_empty() {
        instructions.push(parse_instruction(final_stmt)?);
    }

    Ok(instructions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dockerfile_basic() {
        let content = r#"
# Base image
FROM alpine:3.19

# Set working directory
WORKDIR /app

# Copy files
COPY package.json .
ADD archive.tar.gz /opt/

# Environment variable
ENV PORT=8080
ENV MODE production

# Label and user
LABEL maintainer="test@example.com"
USER node

# Expose port and run commands
EXPOSE 8080/tcp
RUN npm install
ENTRYPOINT ["node"]
CMD ["server.js"]
"#;

        let instructions = parse_dockerfile(content).unwrap();
        assert_eq!(
            instructions,
            vec![
                DockerfileInstruction::From {
                    image: "alpine".to_string(),
                    tag: Some("3.19".to_string()),
                    as_name: None,
                },
                DockerfileInstruction::Workdir("/app".to_string()),
                DockerfileInstruction::Copy {
                    src: "package.json".to_string(),
                    dest: ".".to_string(),
                },
                DockerfileInstruction::Add {
                    src: "archive.tar.gz".to_string(),
                    dest: "/opt/".to_string(),
                },
                DockerfileInstruction::Env {
                    key: "PORT".to_string(),
                    value: "8080".to_string(),
                },
                DockerfileInstruction::Env {
                    key: "MODE".to_string(),
                    value: "production".to_string(),
                },
                DockerfileInstruction::Label {
                    key: "maintainer".to_string(),
                    value: "test@example.com".to_string(),
                },
                DockerfileInstruction::User("node".to_string()),
                DockerfileInstruction::Expose(8080),
                DockerfileInstruction::Run("npm install".to_string()),
                DockerfileInstruction::Entrypoint(vec!["node".to_string()]),
                DockerfileInstruction::Cmd(vec!["server.js".to_string()]),
            ]
        );
    }

    #[test]
    fn test_parse_continuation_lines() {
        let content = r#"
RUN apt-get update && \
    apt-get install -y \
    curl \
    git
"#;

        let instructions = parse_dockerfile(content).unwrap();
        assert_eq!(
            instructions,
            vec![DockerfileInstruction::Run(
                "apt-get update && apt-get install -y curl git".to_string()
            )]
        );
    }

    #[test]
    fn test_parse_from_stages_and_registries() {
        let content = r#"
FROM registry.example.com:5000/myorg/base:v1.2 AS builder
FROM alpine as runner
"#;

        let instructions = parse_dockerfile(content).unwrap();
        assert_eq!(
            instructions,
            vec![
                DockerfileInstruction::From {
                    image: "registry.example.com:5000/myorg/base".to_string(),
                    tag: Some("v1.2".to_string()),
                    as_name: Some("builder".to_string()),
                },
                DockerfileInstruction::From {
                    image: "alpine".to_string(),
                    tag: None,
                    as_name: Some("runner".to_string()),
                },
            ]
        );
    }
}
