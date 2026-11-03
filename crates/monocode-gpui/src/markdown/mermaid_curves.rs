//! Present flowchart routes with Mermaid's smooth basis curves. The pinned
//! renderer exposes routed points but currently emits rounded orthogonal paths.
use mermaid_rs_renderer::layout::Layout;

pub(super) fn flowchart_curves(svg: &mut String, layout: &Layout) {
    for (ix, edge) in layout.edges.iter().enumerate() {
        let Some(curve) = basis_curve(&edge.points) else {
            continue;
        };
        let prefix = format!("<path id=\"edge-{ix}\"");
        let Some(start) = svg.find(&prefix) else {
            continue;
        };
        let Some(len) = svg[start..].find('>') else {
            continue;
        };
        let end = start + len + 1;
        let mut tag = svg[start..end].to_owned();
        let Some(at) = tag.find(" d=\"") else {
            continue;
        };
        let at = at + 4;
        let Some(len) = tag[at..].find('"') else {
            continue;
        };
        tag.replace_range(at..at + len, &curve);
        if edge.override_style.stroke_width.is_none()
            && edge.style == mermaid_rs_renderer::ir::EdgeStyle::Solid
        {
            tag = tag.replacen("stroke-width=\"2\"", "stroke-width=\"1\"", 1);
        }
        svg.replace_range(start..end, &tag);
    }
}

// Open uniform cubic B-spline, as used by d3 curveBasis and the renderer's
// mindmap presentation. Endpoints and their tangents stay at the routed ports.
fn basis_curve(points: &[(f32, f32)]) -> Option<String> {
    let mut points = points.to_vec();
    points.dedup();
    if points.len() < 3 {
        return None;
    }
    let first = points[0];
    let second = points[1];
    let last = *points.last().unwrap();
    let mut path = format!(
        "M {:.3},{:.3} L {:.3},{:.3}",
        first.0,
        first.1,
        (5. * first.0 + second.0) / 6.,
        (5. * first.1 + second.1) / 6.
    );
    points.push(last);
    for segment in points.windows(3) {
        let [a, b, c] = [segment[0], segment[1], segment[2]];
        path.push_str(&format!(
            " C {:.3},{:.3} {:.3},{:.3} {:.3},{:.3}",
            (2. * a.0 + b.0) / 3.,
            (2. * a.1 + b.1) / 3.,
            (a.0 + 2. * b.0) / 3.,
            (a.1 + 2. * b.1) / 3.,
            (a.0 + 4. * b.0 + c.0) / 6.,
            (a.1 + 4. * b.1 + c.1) / 6.
        ));
    }
    path.push_str(&format!(" L {:.3},{:.3}", last.0, last.1));
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_preserve_arrow_endpoints_and_direction() {
        let path = basis_curve(&[(0., 0.), (0., 60.), (120., 60.), (120., 100.)]).unwrap();
        assert!(path.starts_with("M 0.000,0.000 L 0.000,10.000 C "));
        assert!(path.ends_with(" L 120.000,100.000"));
        assert!(basis_curve(&[(0., 0.), (0., 0.), (100., 0.)]).is_none());
    }
}
