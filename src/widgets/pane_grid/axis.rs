use iced::Rectangle;

/// A fixed reference line for the measurement of coordinates.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Axis {
    /// The horizontal axis: —
    Horizontal,
    /// The vertical axis: |
    Vertical,
}

impl Axis {
    /// Splits the provided [`Rectangle`] on the current [`Axis`] with the
    /// given `ratio` and `spacing`.
    pub fn split(
        self,
        rectangle: &Rectangle,
        ratio: f32,
        spacing: f32,
        min_size_a: f32,
        min_size_b: f32,
    ) -> (Rectangle, Rectangle, f32) {
        match self {
            Axis::Horizontal => {
                let height_top = (rectangle.height * ratio - spacing / 2.0)
                    .round()
                    .max(min_size_a)
                    .min(rectangle.height - min_size_b - spacing);

                let height_bottom = (rectangle.height - height_top - spacing).max(min_size_b);

                let ratio = (height_top + spacing / 2.0) / rectangle.height;

                (
                    Rectangle {
                        height: height_top,
                        ..*rectangle
                    },
                    Rectangle {
                        y: rectangle.y + height_top + spacing,
                        height: height_bottom,
                        ..*rectangle
                    },
                    ratio,
                )
            }
            Axis::Vertical => {
                let width_left = (rectangle.width * ratio - spacing / 2.0)
                    .round()
                    .max(min_size_a)
                    .min(rectangle.width - min_size_b - spacing);

                let width_right = (rectangle.width - width_left - spacing).max(min_size_b);

                let ratio = (width_left + spacing / 2.0) / rectangle.width;

                (
                    Rectangle {
                        width: width_left,
                        ..*rectangle
                    },
                    Rectangle {
                        x: rectangle.x + width_left + spacing,
                        width: width_right,
                        ..*rectangle
                    },
                    ratio,
                )
            }
        }
    }

    /// Calculates the bounds of the split line in a [`Rectangle`] region.
    pub fn split_line_bounds(self, rectangle: Rectangle, ratio: f32, spacing: f32) -> Rectangle {
        match self {
            Axis::Horizontal => Rectangle {
                x: rectangle.x,
                y: (rectangle.y + rectangle.height * ratio - spacing / 2.0).round(),
                width: rectangle.width,
                height: spacing,
            },
            Axis::Vertical => Rectangle {
                x: (rectangle.x + rectangle.width * ratio - spacing / 2.0).round(),
                y: rectangle.y,
                width: spacing,
                height: rectangle.height,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct HorizontalCase {
        overall_height: f32,
        spacing: f32,
        top_height: f32,
        bottom_y: f32,
        bottom_height: f32,
    }

    #[derive(Clone, Copy)]
    struct VerticalCase {
        overall_width: f32,
        spacing: f32,
        left_width: f32,
        right_x: f32,
        right_width: f32,
    }

    fn horizontal_cases() -> [HorizontalCase; 4] {
        [
            HorizontalCase {
                overall_height: 10.0,
                spacing: 2.0,
                top_height: 4.0,
                bottom_y: 6.0,
                bottom_height: 4.0,
            },
            HorizontalCase {
                overall_height: 9.0,
                spacing: 2.0,
                top_height: 4.0,
                bottom_y: 6.0,
                bottom_height: 3.0,
            },
            HorizontalCase {
                overall_height: 10.0,
                spacing: 1.0,
                top_height: 5.0,
                bottom_y: 6.0,
                bottom_height: 4.0,
            },
            HorizontalCase {
                overall_height: 9.0,
                spacing: 1.0,
                top_height: 4.0,
                bottom_y: 5.0,
                bottom_height: 4.0,
            },
        ]
    }

    fn vertical_cases() -> [VerticalCase; 4] {
        [
            VerticalCase {
                overall_width: 10.0,
                spacing: 2.0,
                left_width: 4.0,
                right_x: 6.0,
                right_width: 4.0,
            },
            VerticalCase {
                overall_width: 9.0,
                spacing: 2.0,
                left_width: 4.0,
                right_x: 6.0,
                right_width: 3.0,
            },
            VerticalCase {
                overall_width: 10.0,
                spacing: 1.0,
                left_width: 5.0,
                right_x: 6.0,
                right_width: 4.0,
            },
            VerticalCase {
                overall_width: 9.0,
                spacing: 1.0,
                left_width: 4.0,
                right_x: 5.0,
                right_width: 4.0,
            },
        ]
    }

    fn assert_horizontal_split(case: HorizontalCase) {
        let axis = Axis::Horizontal;
        let rectangle = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: case.overall_height,
        };
        let (top, bottom, _ratio) = axis.split(&rectangle, 0.5, case.spacing, 0.0, 0.0);

        assert_eq!(
            top,
            Rectangle {
                height: case.top_height,
                ..rectangle
            }
        );
        assert_eq!(
            bottom,
            Rectangle {
                y: case.bottom_y,
                height: case.bottom_height,
                ..rectangle
            }
        );
    }

    fn assert_vertical_split(case: VerticalCase) {
        let axis = Axis::Vertical;
        let rectangle = Rectangle {
            x: 0.0,
            y: 0.0,
            width: case.overall_width,
            height: 10.0,
        };
        let (left, right, _ratio) = axis.split(&rectangle, 0.5, case.spacing, 0.0, 0.0);

        assert_eq!(
            left,
            Rectangle {
                width: case.left_width,
                ..rectangle
            }
        );
        assert_eq!(
            right,
            Rectangle {
                x: case.right_x,
                width: case.right_width,
                ..rectangle
            }
        );
    }

    #[test]
    fn split_horizontal() {
        for case in horizontal_cases() {
            assert_horizontal_split(case);
        }
    }

    #[test]
    fn split_vertical() {
        for case in vertical_cases() {
            assert_vertical_split(case);
        }
    }
}
