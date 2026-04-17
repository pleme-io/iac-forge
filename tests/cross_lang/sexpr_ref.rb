# frozen_string_literal: true
#
# Reference Ruby implementation of iac-forge's canonical sexpr emission
# and BLAKE3-over-emission content hash.
#
# The Rust implementation owns the contract; this file is the portable
# mirror. Both MUST produce the same hex string for the same canonical
# emission. The cross-language agreement test in tests/cross_language.rs
# pipes Rust-emitted strings through this script and verifies hex match.
#
# Format (exact):
#   Symbol(s)   → s
#   String(s)   → "s" with \\, \", \n, \t escaped
#   Integer(i)  → decimal literal, no decimal point
#   Float(f)    → decimal literal WITH at least one digit after the dot
#                 (plain f.to_s works for Ruby reals — 1.0 is "1.0")
#   Bool(true)  → true
#   Bool(false) → false
#   Nil         → nil
#   List(items) → (item1 item2 ...) space-separated, no trailing space
#
# This file reads EITHER:
#   (a) a single canonical emission from stdin, and prints the hex hash,
#   (b) one emission per line via -lines, printing one hash per line.
#
# Usage:
#   echo '(list integer)' | ruby sexpr_ref.rb
#   cat vectors.txt | ruby sexpr_ref.rb -lines

require 'digest'

# BLAKE3 is supported via the 'blake3' gem when available; fall back to
# a pure-Ruby reference is out of scope. The cross-language test skips
# gracefully if the gem can't load.
begin
  require 'blake3'
rescue LoadError => e
  warn "BLAKE3_UNAVAILABLE: #{e.message}"
  exit 2
end

# Emit a Ruby-array-form of an SExpr back to canonical text.
#
# The input comes in as text already (this reference script just
# re-hashes the text — it doesn't round-trip the SExpr through a Ruby
# AST). The canonical form is defined by the emission, so hashing the
# text IS the contract. The parser below exists only to validate shape.

def hash_hex(text)
  Blake3.hexdigest(text)
end

# Simple shape-validator — parses an emitted sexpr to make sure it's
# well-formed before hashing. Errors on malformed input so we catch
# contract drift early.
class SExprReader
  def initialize(text)
    @chars = text.chars
    @pos = 0
  end

  def read_one
    skip_ws
    c = peek
    case c
    when nil then raise 'unexpected EOF'
    when '(' then read_list
    when '"' then read_string
    else read_atom
    end
  end

  def done?
    skip_ws
    @pos >= @chars.size
  end

  private

  def peek
    @chars[@pos]
  end

  def advance
    c = @chars[@pos]
    @pos += 1
    c
  end

  def skip_ws
    while (c = peek)
      if c =~ /\s/
        advance
      elsif c == ';'
        advance while peek && peek != "\n"
      else
        break
      end
    end
  end

  def read_list
    raise 'expected (' unless advance == '('
    items = []
    loop do
      skip_ws
      case peek
      when nil then raise 'unterminated list'
      when ')' then advance; return items
      else items << read_one
      end
    end
  end

  def read_string
    raise 'expected "' unless advance == '"'
    buf = +''
    loop do
      c = advance
      raise 'unterminated string' if c.nil?
      if c == '"'
        return buf
      elsif c == '\\'
        esc = advance
        buf << case esc
               when 'n' then "\n"
               when 't' then "\t"
               when '"' then '"'
               when '\\' then '\\'
               else raise "unknown escape \\#{esc}"
               end
      else
        buf << c
      end
    end
  end

  def read_atom
    buf = +''
    while (c = peek)
      break if c =~ /\s/ || c == '(' || c == ')' || c == ';' || c == '"'
      buf << c
      advance
    end
    raise 'empty atom' if buf.empty?
    buf
  end
end

if ARGV.include?('-lines')
  $stdin.each_line do |line|
    line = line.chomp
    next if line.empty?
    # Validate shape; hash raw text.
    SExprReader.new(line).read_one
    puts hash_hex(line)
  end
else
  text = $stdin.read
  SExprReader.new(text).read_one
  puts hash_hex(text.chomp)
end
