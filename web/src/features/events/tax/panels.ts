// The keys of the help panels of the tax windows (`Client/SecretRoom{ChangeTaxRate,CheckTaxRate}.inc`, the lower-case stems of `crates/flyff-data/src/client/panels.rs::panel_key`).

export const helpPanelsKey = {
  changeTaxRate: 'secretroomchangetaxrate',
  checkTaxRate: 'secretroomchecktaxrate',
} as const;
