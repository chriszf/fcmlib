//! SVG Path to FCM Outline converter
//!
//! Converts SVG path `d` attribute strings into fcmlib Outline format.
//!
//! # Example
//! ```
//! use fcmlib::svg_path::{SvgPathParser, SvgConfig};
//!
//! let config = SvgConfig {
//!     dpi: 96.0,  // SVG pixels per inch
//!     ..Default::default()
//! };
//!
//! let parser = SvgPathParser::new(config);
//! let paths = parser.parse("M 0,0 L 100,0 L 100,100 Z").unwrap();
//! ```

use crate::{Outline, PathShape, Point, SegmentBezier, SegmentLine};

/// Configuration for SVG to FCM conversion
#[derive(Debug, Clone)]
pub struct SvgConfig {
    /// DPI of the SVG (default 96 for web, 72 for Illustrator)
    pub dpi: f64,
    /// Scale factor (1.0 = no scaling)
    pub scale: f64,
    /// X offset in mm
    pub offset_x_mm: f64,
    /// Y offset in mm
    pub offset_y_mm: f64,
}

impl Default for SvgConfig {
    fn default() -> Self {
        Self {
            dpi: 96.0,
            scale: 1.0,
            offset_x_mm: 0.0,
            offset_y_mm: 0.0,
        }
    }
}

impl SvgConfig {
    /// Convert SVG coordinate to FCM units (hundredths of mm)
    pub fn to_fcm(&self, svg_value: f64) -> i32 {
        // SVG pixels → inches → mm → hundredths of mm
        let inches = svg_value / self.dpi;
        let mm = inches * 25.4 * self.scale;
        (mm * 100.0).round() as i32
    }

    /// Convert SVG point to FCM Point
    pub fn point_to_fcm(&self, x: f64, y: f64) -> Point {
        Point {
            x: self.to_fcm(x) + (self.offset_x_mm * 100.0) as i32,
            y: self.to_fcm(y) + (self.offset_y_mm * 100.0) as i32,
        }
    }
}

/// SVG Path parser and converter
pub struct SvgPathParser {
    config: SvgConfig,
}

/// Represents a parsed SVG subpath (one continuous path from M to Z or next M)
#[derive(Debug, Clone)]
pub struct ParsedSubpath {
    pub start: Point,
    pub outline: Outline,
    pub closed: bool,
}

/// Error type for SVG parsing
#[derive(Debug, Clone)]
pub struct SvgParseError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for SvgParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SVG parse error at {}: {}", self.position, self.message)
    }
}

impl std::error::Error for SvgParseError {}

impl SvgPathParser {
    pub fn new(config: SvgConfig) -> Self {
        Self { config }
    }

    /// Parse an SVG path `d` attribute into FCM PathShapes
    pub fn parse(&self, d: &str) -> Result<Vec<PathShape>, SvgParseError> {
        let subpaths = self.parse_to_subpaths(d)?;

        Ok(subpaths
            .into_iter()
            .map(|sp| PathShape {
                start: sp.start,
                outlines: vec![sp.outline],
            })
            .collect())
    }

    /// Parse SVG path into subpaths (more detailed output)
    pub fn parse_to_subpaths(&self, d: &str) -> Result<Vec<ParsedSubpath>, SvgParseError> {
        let tokens = tokenize(d)?;
        self.parse_tokens(&tokens)
    }

    fn parse_tokens(&self, tokens: &[Token]) -> Result<Vec<ParsedSubpath>, SvgParseError> {
        let mut subpaths = Vec::new();
        let mut current_x = 0.0f64;
        let mut current_y = 0.0f64;
        let mut subpath_start_x = 0.0f64;
        let mut subpath_start_y = 0.0f64;
        let mut current_segments: Vec<Segment> = Vec::new();
        let mut has_start = false;

        // For smooth curve continuations
        let mut last_control_x = 0.0f64;
        let mut last_control_y = 0.0f64;
        let mut last_command = ' ';

        let mut i = 0;
        while i < tokens.len() {
            match &tokens[i] {
                Token::Command(cmd) => {
                    let cmd_char = *cmd;
                    let is_relative = cmd_char.is_lowercase();
                    let cmd_upper = cmd_char.to_ascii_uppercase();

                    i += 1;

                    match cmd_upper {
                        'M' => {
                            // MoveTo - starts a new subpath
                            if has_start && !current_segments.is_empty() {
                                subpaths.push(self.build_subpath(
                                    subpath_start_x,
                                    subpath_start_y,
                                    &current_segments,
                                    false,
                                ));
                                current_segments.clear();
                            }

                            let (x, y, consumed) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                            i += consumed;

                            subpath_start_x = x;
                            subpath_start_y = y;
                            current_x = x;
                            current_y = y;
                            has_start = true;

                            // Additional coordinate pairs are treated as LineTo
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let (x, y, consumed) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed;
                                current_segments.push(Segment::Line { x, y });
                                current_x = x;
                                current_y = y;
                            }
                        }

                        'L' => {
                            // LineTo
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let (x, y, consumed) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed;
                                current_segments.push(Segment::Line { x, y });
                                current_x = x;
                                current_y = y;
                            }
                        }

                        'H' => {
                            // Horizontal LineTo
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let x = self.read_number(&tokens[i])?;
                                i += 1;
                                let x = if is_relative { current_x + x } else { x };
                                current_segments.push(Segment::Line { x, y: current_y });
                                current_x = x;
                            }
                        }

                        'V' => {
                            // Vertical LineTo
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let y = self.read_number(&tokens[i])?;
                                i += 1;
                                let y = if is_relative { current_y + y } else { y };
                                current_segments.push(Segment::Line { x: current_x, y });
                                current_y = y;
                            }
                        }

                        'C' => {
                            // Cubic Bezier
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let (c1x, c1y, consumed1) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed1;
                                let (c2x, c2y, consumed2) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed2;
                                let (x, y, consumed3) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed3;

                                current_segments.push(Segment::Cubic { c1x, c1y, c2x, c2y, x, y });
                                last_control_x = c2x;
                                last_control_y = c2y;
                                current_x = x;
                                current_y = y;
                            }
                        }

                        'S' => {
                            // Smooth Cubic Bezier
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                // First control point is reflection of last control point
                                let c1x = if last_command == 'C' || last_command == 'S' {
                                    2.0 * current_x - last_control_x
                                } else {
                                    current_x
                                };
                                let c1y = if last_command == 'C' || last_command == 'S' {
                                    2.0 * current_y - last_control_y
                                } else {
                                    current_y
                                };

                                let (c2x, c2y, consumed1) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed1;
                                let (x, y, consumed2) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed2;

                                current_segments.push(Segment::Cubic { c1x, c1y, c2x, c2y, x, y });
                                last_control_x = c2x;
                                last_control_y = c2y;
                                current_x = x;
                                current_y = y;
                            }
                        }

                        'Q' => {
                            // Quadratic Bezier - convert to cubic
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let (qx, qy, consumed1) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed1;
                                let (x, y, consumed2) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed2;

                                // Convert quadratic to cubic bezier
                                let c1x = current_x + (2.0 / 3.0) * (qx - current_x);
                                let c1y = current_y + (2.0 / 3.0) * (qy - current_y);
                                let c2x = x + (2.0 / 3.0) * (qx - x);
                                let c2y = y + (2.0 / 3.0) * (qy - y);

                                current_segments.push(Segment::Cubic { c1x, c1y, c2x, c2y, x, y });
                                last_control_x = qx;
                                last_control_y = qy;
                                current_x = x;
                                current_y = y;
                            }
                        }

                        'T' => {
                            // Smooth Quadratic Bezier
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let qx = if last_command == 'Q' || last_command == 'T' {
                                    2.0 * current_x - last_control_x
                                } else {
                                    current_x
                                };
                                let qy = if last_command == 'Q' || last_command == 'T' {
                                    2.0 * current_y - last_control_y
                                } else {
                                    current_y
                                };

                                let (x, y, consumed) = self.read_point(&tokens[i..], is_relative, current_x, current_y)?;
                                i += consumed;

                                // Convert quadratic to cubic
                                let c1x = current_x + (2.0 / 3.0) * (qx - current_x);
                                let c1y = current_y + (2.0 / 3.0) * (qy - current_y);
                                let c2x = x + (2.0 / 3.0) * (qx - x);
                                let c2y = y + (2.0 / 3.0) * (qy - y);

                                current_segments.push(Segment::Cubic { c1x, c1y, c2x, c2y, x, y });
                                last_control_x = qx;
                                last_control_y = qy;
                                current_x = x;
                                current_y = y;
                            }
                        }

                        'A' => {
                            // Arc - convert to cubic bezier approximation
                            while i < tokens.len() && matches!(&tokens[i], Token::Number(_)) {
                                let rx = self.read_number(&tokens[i])?;
                                let ry = self.read_number(&tokens[i + 1])?;
                                let x_rotation = self.read_number(&tokens[i + 2])?;
                                let large_arc = self.read_number(&tokens[i + 3])? != 0.0;
                                let sweep = self.read_number(&tokens[i + 4])? != 0.0;
                                let (x, y, _) = self.read_point(&tokens[i + 5..], is_relative, current_x, current_y)?;
                                i += 7;

                                let arc_segments = arc_to_beziers(
                                    current_x, current_y,
                                    rx, ry,
                                    x_rotation,
                                    large_arc, sweep,
                                    x, y,
                                );

                                for seg in arc_segments {
                                    current_segments.push(seg);
                                }

                                current_x = x;
                                current_y = y;
                            }
                        }

                        'Z' => {
                            // ClosePath
                            if has_start && !current_segments.is_empty() {
                                // Add closing line if needed
                                if (current_x - subpath_start_x).abs() > 0.001
                                    || (current_y - subpath_start_y).abs() > 0.001
                                {
                                    current_segments.push(Segment::Line {
                                        x: subpath_start_x,
                                        y: subpath_start_y,
                                    });
                                }

                                subpaths.push(self.build_subpath(
                                    subpath_start_x,
                                    subpath_start_y,
                                    &current_segments,
                                    true,
                                ));
                                current_segments.clear();
                            }

                            current_x = subpath_start_x;
                            current_y = subpath_start_y;
                            has_start = false;
                        }

                        _ => {
                            return Err(SvgParseError {
                                message: format!("Unknown command: {}", cmd_char),
                                position: i,
                            });
                        }
                    }

                    last_command = cmd_upper;
                }

                Token::Number(_) => {
                    return Err(SvgParseError {
                        message: "Unexpected number without command".to_string(),
                        position: i,
                    });
                }
            }
        }

        // Handle unclosed path
        if has_start && !current_segments.is_empty() {
            subpaths.push(self.build_subpath(
                subpath_start_x,
                subpath_start_y,
                &current_segments,
                false,
            ));
        }

        Ok(subpaths)
    }

    fn read_number(&self, token: &Token) -> Result<f64, SvgParseError> {
        match token {
            Token::Number(n) => Ok(*n),
            Token::Command(c) => Err(SvgParseError {
                message: format!("Expected number, got command '{}'", c),
                position: 0,
            }),
        }
    }

    fn read_point(
        &self,
        tokens: &[Token],
        is_relative: bool,
        current_x: f64,
        current_y: f64,
    ) -> Result<(f64, f64, usize), SvgParseError> {
        if tokens.len() < 2 {
            return Err(SvgParseError {
                message: "Not enough values for point".to_string(),
                position: 0,
            });
        }

        let x = self.read_number(&tokens[0])?;
        let y = self.read_number(&tokens[1])?;

        let (x, y) = if is_relative {
            (current_x + x, current_y + y)
        } else {
            (x, y)
        };

        Ok((x, y, 2))
    }

    fn build_subpath(
        &self,
        start_x: f64,
        start_y: f64,
        segments: &[Segment],
        closed: bool,
    ) -> ParsedSubpath {
        let start = self.config.point_to_fcm(start_x, start_y);

        // Check if all segments are lines or if we have beziers
        let has_beziers = segments.iter().any(|s| matches!(s, Segment::Cubic { .. }));

        let outline = if has_beziers {
            // Convert all to bezier format
            Outline::Bezier(
                segments
                    .iter()
                    .map(|seg| match seg {
                        Segment::Line { x, y } => {
                            // Line as degenerate bezier (control points on the line)
                            let end = self.config.point_to_fcm(*x, *y);
                            SegmentBezier {
                                control1: end,
                                control2: end,
                                end,
                            }
                        }
                        Segment::Cubic { c1x, c1y, c2x, c2y, x, y } => SegmentBezier {
                            control1: self.config.point_to_fcm(*c1x, *c1y),
                            control2: self.config.point_to_fcm(*c2x, *c2y),
                            end: self.config.point_to_fcm(*x, *y),
                        },
                    })
                    .collect(),
            )
        } else {
            // All lines
            Outline::Line(
                segments
                    .iter()
                    .map(|seg| match seg {
                        Segment::Line { x, y } => SegmentLine {
                            end: self.config.point_to_fcm(*x, *y),
                        },
                        _ => unreachable!(),
                    })
                    .collect(),
            )
        };

        ParsedSubpath {
            start,
            outline,
            closed,
        }
    }
}

// Internal segment representation
#[derive(Debug, Clone)]
enum Segment {
    Line { x: f64, y: f64 },
    Cubic { c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64 },
}

// Token types for path parsing
#[derive(Debug, Clone)]
enum Token {
    Command(char),
    Number(f64),
}

/// Tokenize an SVG path string
fn tokenize(d: &str) -> Result<Vec<Token>, SvgParseError> {
    let mut tokens = Vec::new();
    let mut chars = d.chars().peekable();
    let mut pos = 0;

    while let Some(&c) = chars.peek() {
        match c {
            // Whitespace and comma separators
            ' ' | '\t' | '\n' | '\r' | ',' => {
                chars.next();
                pos += 1;
            }

            // Commands
            'M' | 'm' | 'L' | 'l' | 'H' | 'h' | 'V' | 'v' | 'C' | 'c' | 'S' | 's' | 'Q' | 'q'
            | 'T' | 't' | 'A' | 'a' | 'Z' | 'z' => {
                tokens.push(Token::Command(c));
                chars.next();
                pos += 1;
            }

            // Numbers (including negative and decimals)
            '-' | '+' | '.' | '0'..='9' => {
                let mut num_str = String::new();

                // Sign
                if c == '-' || c == '+' {
                    num_str.push(c);
                    chars.next();
                    pos += 1;
                }

                // Integer part
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        chars.next();
                        pos += 1;
                    } else {
                        break;
                    }
                }

                // Decimal part
                if let Some(&'.') = chars.peek() {
                    num_str.push('.');
                    chars.next();
                    pos += 1;

                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() {
                            num_str.push(c);
                            chars.next();
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                }

                // Exponent part
                if let Some(&c) = chars.peek() {
                    if c == 'e' || c == 'E' {
                        num_str.push(c);
                        chars.next();
                        pos += 1;

                        if let Some(&c) = chars.peek() {
                            if c == '-' || c == '+' {
                                num_str.push(c);
                                chars.next();
                                pos += 1;
                            }
                        }

                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                num_str.push(c);
                                chars.next();
                                pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }

                let num: f64 = num_str.parse().map_err(|_| SvgParseError {
                    message: format!("Invalid number: {}", num_str),
                    position: pos,
                })?;

                tokens.push(Token::Number(num));
            }

            _ => {
                return Err(SvgParseError {
                    message: format!("Unexpected character: '{}'", c),
                    position: pos,
                });
            }
        }
    }

    Ok(tokens)
}

/// Convert an arc to cubic bezier segments
fn arc_to_beziers(
    x1: f64, y1: f64,
    mut rx: f64, mut ry: f64,
    x_rotation: f64,
    large_arc: bool, sweep: bool,
    x2: f64, y2: f64,
) -> Vec<Segment> {
    // Handle degenerate cases
    if (x1 - x2).abs() < 1e-10 && (y1 - y2).abs() < 1e-10 {
        return vec![];
    }

    if rx.abs() < 1e-10 || ry.abs() < 1e-10 {
        return vec![Segment::Line { x: x2, y: y2 }];
    }

    rx = rx.abs();
    ry = ry.abs();

    let phi = x_rotation.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();

    // Step 1: Compute (x1', y1')
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    // Scale radii if needed
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let sqrt_lambda = lambda.sqrt();
        rx *= sqrt_lambda;
        ry *= sqrt_lambda;
    }

    // Step 2: Compute (cx', cy')
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;

    let mut sq = ((rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2) / (rx2 * y1p2 + ry2 * x1p2)).max(0.0);
    sq = sq.sqrt();

    if large_arc == sweep {
        sq = -sq;
    }

    let cxp = sq * rx * y1p / ry;
    let cyp = -sq * ry * x1p / rx;

    // Step 3: Compute (cx, cy)
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    // Step 4: Compute angles
    let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dtheta = angle(
        (x1p - cxp) / rx, (y1p - cyp) / ry,
        (-x1p - cxp) / rx, (-y1p - cyp) / ry,
    );

    if !sweep && dtheta > 0.0 {
        dtheta -= 2.0 * std::f64::consts::PI;
    } else if sweep && dtheta < 0.0 {
        dtheta += 2.0 * std::f64::consts::PI;
    }

    // Convert arc to bezier curves
    let n_curves = (dtheta.abs() / (std::f64::consts::PI / 2.0)).ceil() as usize;
    let dtheta_per_curve = dtheta / n_curves as f64;

    let mut segments = Vec::new();
    let mut current_theta = theta1;

    for _ in 0..n_curves {
        let theta_end = current_theta + dtheta_per_curve;

        // Bezier control point calculation for arc segment
        let t = (dtheta_per_curve / 4.0).tan();
        let alpha = (dtheta_per_curve).sin() * ((4.0 + 3.0 * t * t).sqrt() - 1.0) / 3.0;

        let cos_t1 = current_theta.cos();
        let sin_t1 = current_theta.sin();
        let cos_t2 = theta_end.cos();
        let sin_t2 = theta_end.sin();

        let p1x = cx + rx * cos_phi * cos_t1 - ry * sin_phi * sin_t1;
        let p1y = cy + rx * sin_phi * cos_t1 + ry * cos_phi * sin_t1;

        let p2x = cx + rx * cos_phi * cos_t2 - ry * sin_phi * sin_t2;
        let p2y = cy + rx * sin_phi * cos_t2 + ry * cos_phi * sin_t2;

        let dx1 = -rx * cos_phi * sin_t1 - ry * sin_phi * cos_t1;
        let dy1 = -rx * sin_phi * sin_t1 + ry * cos_phi * cos_t1;

        let dx2 = -rx * cos_phi * sin_t2 - ry * sin_phi * cos_t2;
        let dy2 = -rx * sin_phi * sin_t2 + ry * cos_phi * cos_t2;

        segments.push(Segment::Cubic {
            c1x: p1x + alpha * dx1,
            c1y: p1y + alpha * dy1,
            c2x: p2x - alpha * dx2,
            c2y: p2y - alpha * dy2,
            x: p2x,
            y: p2y,
        });

        current_theta = theta_end;
    }

    segments
}

/// Calculate angle between two vectors
fn angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    let mut angle = (dot / len).clamp(-1.0, 1.0).acos();

    if ux * vy - uy * vx < 0.0 {
        angle = -angle;
    }

    angle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rect() {
        let parser = SvgPathParser::new(SvgConfig {
            dpi: 96.0,
            ..Default::default()
        });

        let result = parser.parse("M 0,0 L 100,0 L 100,100 L 0,100 Z").unwrap();
        assert_eq!(result.len(), 1);

        let shape = &result[0];
        assert_eq!(shape.start.x, 0);
        assert_eq!(shape.start.y, 0);

        match &shape.outlines[0] {
            Outline::Line(segments) => {
                assert_eq!(segments.len(), 4); // 3 lines + closing line
            }
            _ => panic!("Expected line outline"),
        }
    }

    #[test]
    fn test_relative_commands() {
        let parser = SvgPathParser::new(SvgConfig {
            dpi: 96.0,
            ..Default::default()
        });

        let result = parser.parse("M 0,0 l 100,0 l 0,100 l -100,0 z").unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_bezier_curve() {
        let parser = SvgPathParser::new(SvgConfig {
            dpi: 96.0,
            ..Default::default()
        });

        let result = parser.parse("M 0,0 C 50,0 50,100 100,100").unwrap();
        assert_eq!(result.len(), 1);

        match &result[0].outlines[0] {
            Outline::Bezier(segments) => {
                assert_eq!(segments.len(), 1);
            }
            _ => panic!("Expected bezier outline"),
        }
    }
}
