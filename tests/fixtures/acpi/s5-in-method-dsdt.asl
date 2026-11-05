/*
 * Source of s5-in-method-dsdt.aml (76 bytes, hand-assembled: iasl was not
 * available when the fixture was made; the AML bytes follow ACPI 6.5 §20).
 *
 * The only `\_S5_` object is created inside a method body, so it exists only
 * while `_PTS` runs. A static `\_S5_` scan must not accept it, even though the
 * bytes `08 5C 5F 53 35 5F 12` (NameOp `\_S5_` PackageOp) appear verbatim.
 * The static `_S3_` package at the root is a decoy with the same shape.
 */
DefinitionBlock ("", "DSDT", 2, "CELLOS", "S5METHOD", 0x00000001)
{
    Scope (\)
    {
        Name (_S3_, Package (0x04) { 0x05, 0x05, Zero, Zero })
    }

    Method (_PTS, 1, NotSerialized)
    {
        Name (\_S5_, Package (0x04) { 0x05, 0x05, Zero, Zero })
    }
}
