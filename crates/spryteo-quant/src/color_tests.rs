    use super::*;

    #[test]
    fn srgb_to_lab_black() {
        let lab = srgb_to_lab(&Rgb { r: 0, g: 0, b: 0 });
        assert!(lab.l.abs() < 1.0);
        assert!(lab.a.abs() < 1.0);
        assert!(lab.b.abs() < 1.0);
    }

    #[test]
    fn srgb_to_lab_white() {
        let lab = srgb_to_lab(&Rgb {
            r: 255,
            g: 255,
            b: 255,
        });
        assert!((lab.l - 100.0).abs() < 1.0);
    }

    #[test]
    fn round_trip_preserves_colour() {
        let input = Rgb {
            r: 123,
            g: 67,
            b: 200,
        };
        let lab = srgb_to_lab(&input);
        let output = lab_to_srgb(&lab);
        // Round-trip should be lossy within a couple of code values
        let dr = (input.r as i16 - output.r as i16).abs();
        let dg = (input.g as i16 - output.g as i16).abs();
        let db = (input.b as i16 - output.b as i16).abs();
        assert!(dr <= 2, "r diff: {dr}");
        assert!(dg <= 2, "g diff: {dg}");
        assert!(db <= 2, "b diff: {db}");
    }
