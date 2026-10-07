//! The questions `setup` asks on a terminal, each with a default that Enter
//! takes. Reading and writing go through the arguments, so tests can answer.

use std::io::{BufRead, Write};

use anyhow::{Result, bail};
use routedroid_helper_ipc::Interface;

use super::block::Block;
use super::choose::{Choice, candidates, describe, none_usable, preferred};

pub fn ask(
    interfaces: &[Interface],
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<Choice> {
    let candidates = candidates(interfaces);
    if candidates.is_empty() {
        return Err(none_usable(interfaces));
    }
    writeln!(out, "Interfaces phones could join the LAN through:")?;
    for link in &candidates {
        writeln!(out, "  {}", describe(link))?;
    }
    let link = match preferred(&candidates) {
        Some(link)
            if yes_no(
                input,
                out,
                &format!("Let phones join the LAN through {}?", link.name),
                true,
            )? =>
        {
            link
        }
        _ => which(&candidates, input, out)?,
    };
    let dhcp = yes_no(
        input,
        out,
        "Lease phone addresses from the LAN's DHCP server (recommended)?",
        true,
    )?;
    let blocks = if dhcp {
        Vec::new()
    } else {
        vec![block(link, input, out)?]
    };
    Ok(Choice {
        lan_if: link.name.clone(),
        dhcp,
        blocks,
    })
}

fn which<'a>(
    candidates: &[&'a Interface],
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<&'a Interface> {
    let names: Vec<&str> = candidates.iter().map(|link| link.name.as_str()).collect();
    loop {
        let answer = line(input, out, &format!("Which one? ({}): ", names.join(", ")))?;
        match candidates.iter().find(|link| link.name == answer) {
            Some(link) => return Ok(link),
            None => writeln!(out, "  {answer:?} is not one of them")?,
        }
    }
}

fn block(link: &Interface, input: &mut impl BufRead, out: &mut impl Write) -> Result<Block> {
    let example = Block::example(link)
        .map(|b| format!(" (e.g. {b})"))
        .unwrap_or_default();
    let prompt = format!(
        "Addresses phones may take on {}, as a block{example}: ",
        link.name
    );
    loop {
        match line(input, out, &prompt)?.parse::<Block>() {
            Ok(block) if block.inside(link) => return Ok(block),
            Ok(block) => writeln!(out, "  {block} is not on {}'s LAN", link.name)?,
            Err(why) => writeln!(out, "  {why}")?,
        }
    }
}

fn yes_no(
    input: &mut impl BufRead,
    out: &mut impl Write,
    question: &str,
    default: bool,
) -> Result<bool> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        match line(input, out, &format!("{question} {hint} "))?
            .to_lowercase()
            .as_str()
        {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(out, "  answer y or n")?,
        }
    }
}

/// One trimmed answer; the input ending is an error, not an endless loop.
fn line(input: &mut impl BufRead, out: &mut impl Write, prompt: &str) -> Result<String> {
    write!(out, "{prompt}")?;
    out.flush()?;
    let mut answer = String::new();
    if input.read_line(&mut answer)? == 0 {
        bail!("no answer: the input ended");
    }
    Ok(answer.trim().to_string())
}
