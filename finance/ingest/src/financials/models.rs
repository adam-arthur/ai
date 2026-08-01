use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumIter, EnumString};

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[allow(clippy::upper_case_acronyms)]
pub enum Exchange {
    AMEX,
    ARCA,
    BATS,
    NYSE,
    NASDAQ,
    NYSEARCA,
    OTC,
}

impl Exchange {
    #[allow(dead_code)]
    fn as_value(&self) -> String {
        match self {
            Exchange::AMEX => "AMEX".into(),
            Exchange::ARCA => "ARCA".into(),
            Exchange::BATS => "BATS".into(),
            Exchange::NYSE => "NYSE".into(),
            Exchange::NASDAQ => "NASDAQ".into(),
            Exchange::NYSEARCA => "NYSEARCA".into(),
            Exchange::OTC => "OTC".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SymbolMeta {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cik: Option<String>,
    pub name: String,
    pub exchange: Exchange,
    pub is_easy_to_borrow: bool,
    pub is_shortable: bool,
    pub is_fractionable: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TreasuryRate {
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,
}

#[derive(EnumIter, EnumString, Display, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum TreasuryDuration {
    #[serde(rename = "1mo")]
    OneMonth,
    #[serde(rename = "3mo")]
    ThreeMonth,
    #[serde(rename = "6mo")]
    SixMonth,
    #[serde(rename = "1y")]
    OneYear,
    #[serde(rename = "2y")]
    TwoYear,
    #[serde(rename = "3y")]
    ThreeYear,
    #[serde(rename = "5y")]
    FiveYear,
    #[serde(rename = "7y")]
    SevenYear,
    #[serde(rename = "10y")]
    TenYear,
    #[serde(rename = "20y")]
    TwentyYear,
    #[serde(rename = "30y")]
    ThirtyYear,
}

impl TreasuryDuration {
    pub fn as_value(&self) -> String {
        match self {
            TreasuryDuration::OneMonth => "1mo",
            TreasuryDuration::ThreeMonth => "3mo",
            TreasuryDuration::SixMonth => "6mo",
            TreasuryDuration::OneYear => "1y",
            TreasuryDuration::TwoYear => "2y",
            TreasuryDuration::ThreeYear => "3y",
            TreasuryDuration::FiveYear => "5y",
            TreasuryDuration::SevenYear => "7y",
            TreasuryDuration::TenYear => "10y",
            TreasuryDuration::TwentyYear => "20y",
            TreasuryDuration::ThirtyYear => "30y",
        }
        .into()
    }

    pub fn as_series_name(&self) -> String {
        match self {
            TreasuryDuration::OneMonth => "DGS1MO",
            TreasuryDuration::ThreeMonth => "DGS3MO",
            TreasuryDuration::SixMonth => "DGS6MO",
            TreasuryDuration::OneYear => "DGS1",
            TreasuryDuration::TwoYear => "DGS2",
            TreasuryDuration::ThreeYear => "DGS3",
            TreasuryDuration::FiveYear => "DGS5",
            TreasuryDuration::SevenYear => "DGS7",
            TreasuryDuration::TenYear => "DGS10",
            TreasuryDuration::TwentyYear => "DGS20",
            TreasuryDuration::ThirtyYear => "DGS30",
        }
        .into()
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(EnumIter, EnumString, Display, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Currency {
    CAD,
    EUR,
    GBP,
    HKD,
    JPY,
    USD,
}

#[allow(dead_code)]
impl Currency {
    pub fn get_foreign_currencies() -> Vec<Currency> {
        vec![
            Currency::CAD,
            Currency::EUR,
            Currency::GBP,
            Currency::HKD,
            Currency::JPY,
        ]
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExchangeRate {
    pub from: Currency,
    pub to: Currency,
    pub rate: f64,
}

// TODO: Pull from other project
// TODO: Add special sector codes for CEF/BDC etc
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Sector {
    pub symbol: String,
    // TODO: Type this more formally
    // sectorName: 'Energy' | 'Materials' | 'Industrials' | 'Consumer Discretionary' | 'Consumer Staples' | 'Health Care' | 'Financials' | 'Information Technology' | 'Communication Services' | 'Utilities' | 'Real Estate'
    // pub sector_name: String,
    // pub sector_gics: u32,

    // industryGroupName: 'Energy' | 'Materials' | 'Capital goods' | 'Commercial & Professional Services' | 'Transportation' | 'Automobiles & Components' | 'Consumer Durables & Apparel' | 'Consumer Services' | 'Retailing' | 'Food & Staples Retailing' | 'Food | Beverage & Tobacco' | 'Household & Personal Products' | 'Health Care Equipment & Services' | 'Pharmaceuticals | Biotechnology & Life' | 'Banks' | 'Diversified Financials' | 'Insurance' | 'Software & Services' | 'Technology Hardware & Equipment' | 'Semiconductors & Semiconductor' | 'Telecommunication Services' | 'Media & Entertainment' | 'Utilities' | 'Real Estate'
    // pub industry_group_name: String,
    // pub industry_group_gics: u32,

    // industryName: 'Energy Equipment & Services' | 'Oil, Gas & Consumable Fuels' | 'Chemicals' | 'Construction Materials' | 'Containers & Packaging' | 'Metals & Mining' | 'Paper & Forest Products' | 'Aerospace & Defense' | 'Building Products' | 'Construction & Engineering' | 'Electrical Equipment' | 'Industrial Conglomerates' | 'Machinery' | 'Trading Companies & Distributors' | 'Commercial Services & Supplies' | 'Professional Services' | 'Air Freight & Logistics' | 'Airlines' | 'Marine' | 'Road & Rail' | 'Transportation Infrastructure' | 'Auto Components    ' | 'Automobiles' | 'Household Durables' | 'Leisure Products' | 'Textiles, Apparel & Luxury Goods' | 'Hotels, Restaurants & Leisure,' | 'Diversified Consumer Services' | 'Distributors' | 'Internet & Direct Marketing Retail' | 'Multiline Retail' | 'Specialty Retail' | 'Food & Staples Retailing' | 'Beverages' | 'Food Products' | 'Tobacco' | 'Household Products' | 'Personal Products' | 'Health Care Equipment & Supplies' | 'Health Care Providers & Services' | 'Health Care Technology' | 'Biotechnology' | 'Pharmaceuticals' | 'Life Sciences Tools & Services' | 'Banks' | 'Thrifts & Mortgage Finance' | 'Diversified Financial Services' | 'Consumer Finance' | 'Capital Markets' | 'Mortgage Real Estate Investment Trusts (REITs)' | 'Insurance' | 'IT Services' | 'Software' | 'Communications Equipment' | 'Technology Hardware, Storage & Peripherals' | 'Electronic Equipment, Instruments & Components' | 'Semiconductors & Semiconductor Equipment' | 'Diversified Telecommunication Services' | 'Wireless Telecommunication Services' | 'Media' | 'Entertainment' | 'Interactive Media & Services' | 'Electric Utilities' | 'Gas Utilities' | 'Multi-Utilities' | 'Water Utilities' | 'Independent Power and Renewable Electricity Producers' | 'Equity Real Estate Investment Trusts (REITs)' | 'Real Estate Management & Development'
    // pub industry_name: String,
    // pub industry_gics: u32,

    // sub_industry_name: 'Oil & Gas Drilling' | 'Oil & Gas Equipment & Services' | 'Integrated Oil & Gas' | 'Oil & Gas Exploration & Production' | 'Oil & Gas Refining & Marketing' | 'Oil & Gas Storage & Transportation' | 'Coal & Consumable Fuels' | 'Commodity Chemicals' | 'Diversified Chemicals' | 'Fertilizers & Agricultural Chemicals' | 'Industrial Gases' | 'Specialty Chemicals' | 'Construction Materials' | 'Metal & Glass Containers' | 'Paper Packaging' | 'Aluminum' | 'Diversified Metals & Mining' | 'Copper' | 'Gold' | 'Precious Metals & Minerals' | 'Silver' | 'Steel' | 'Forest Products' | 'Paper Products' | 'Aerospace & Defense' | 'Building Products' | 'Construction & Engineering' | 'Electrical Components & Equipment' | 'Heavy Electrical Equipment' | 'Industrial Conglomerates' | 'Construction Machinery & Heavy Trucks' | 'Agricultural & Farm Machinery' | 'Industrial Machinery' | 'Trading Companies & Distributors' | 'Commercial Printing' | 'Environmental & Facilities Services' | 'Office Services & Supplies' | 'Diversified Support Services' | 'Security & Alarm Services' | 'Human Resource & Employment Services' | 'Research & Consulting Services' | 'Air Freight & Logistics' | 'Airlines' | 'Marine' | 'Railroads' | 'Trucking' | 'Airport Services' | 'Highways & Railtracks' | 'Marine Ports & Services' | 'Auto Parts & Equipment' | 'Tires & Rubber' | 'Automobile Manufacturers' | 'Motorcycle Manufacturers' | 'Consumer Electronics' | 'Home Furnishings' | 'Homebuilding' | 'Household Appliances' | 'Housewares & Specialties' | 'Leisure Products' | 'Apparel, Accessories & Luxury Goods' | 'Footwear' | 'Textiles' | 'Casinos & Gaming' | 'Hotels, Resorts & Cruise Lines' | 'Leisure Facilities' | 'Restaurants' | 'Education Services' | 'Specialized Consumer Services' | 'Distributors' | 'Internet & Direct Marketing Retail' | 'Department Stores' | 'General Merchandise Stores' | 'Apparel Retail' | 'Computer & Electronics Retail' | 'Home Improvement Retail' | 'Specialty Stores' | 'Automotive Retail' | 'Homefurnishing Retail' | 'Drug Retail' | 'Food Distributors' | 'Food Retail' | 'Hypermarkets & Super Centers' | 'Brewers' | 'Distillers & Vintners' | 'Soft Drinks' | 'Agricultural Products' | 'Packaged Foods & Meats' | 'Tobacco' | 'Household Products' | 'Personal Products' | 'Health Care Equipment' | 'Health Care Supplies' | 'Health Care Distributors' | 'Health Care Services' | 'Health Care Facilities' | 'Managed Health Care' | 'Health Care Technology' | 'Biotechnology' | 'Pharmaceuticals' | 'Life Sciences Tools & Services' | 'Diversified Banks' | 'Regional Banks' | 'Thrifts & Mortgage Finance' | 'Other Diversified Financial Services' | 'Multi-Sector Holdings' | 'Specialized Finance' | 'Consumer Finance' | 'Asset Management & Custody Banks' | 'Investment Banking & Brokerage' | 'Diversified Capital Markets' | 'Financial Exchanges & Data' | 'Mortgage REITs' | 'Insurance Brokers' | 'Life & Health Insurance' | 'Multi-line Insurance' | 'Property & Casualty Insurance' | 'Reinsurance' | 'IT Consulting & Other Services' | 'Data Processing & Outsourced Services' | 'Internet Services & Infrastructure' | 'Application Software' | 'Systems Software' | 'Communications Equipment' | 'Technology Hardware, Storage & Peripherals' | 'Electronic Equipment & Instruments' | 'Electronic Components' | 'Electronic Manufacturing Services' | 'Semiconductor Equipment' | 'Semiconductors' | 'Alternative Carriers' | 'Integrated Telecommunication Services' | 'Wireless Telecommunication Services' | 'Advertising' | 'Broadcasting' | 'Cable & Satellite' | 'Publishing' | 'Movies & Entertainment' | 'Interactive Home Entertainment' | 'Interactive Media & Services' | 'Electric Utilities' | 'Gas Utilities' | 'Multi-Utilities' | 'Water Utilities' | 'Independent Power Producers & Energy Traders' | 'Renewable Electricity' | 'Diversified REITs' | 'Industrial REITs' | 'Hotel & Resort REITs' | 'Office REITs' | 'Health Care REITs' | 'Residential REITs' | 'Retail REITs' | 'Specialized REITs' | 'Diversified Real Estate Activities' | 'Real Estate Operating Companies' | 'Real Estate Development' | 'Real Estate Services'
    // pub sub_industry_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_industry_gics: Option<u64>,
}

#[rustfmt::skip]
#[allow(dead_code)]
pub static SECTORID_TO_NAME: Lazy<HashMap<u32, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(10, "Energy");
        m.insert(1010, "Energy");
            m.insert(101010, "Energy Equipment & Services");
                m.insert(10101010, "Oil & Gas Drilling");
                m.insert(10101020, "Oil & Gas Equipment & Services");
            m.insert(101020, "Oil, Gas & Consumable Fuels");
                m.insert(10102010, "Integrated Oil & Gas");
                m.insert(10102020, "Oil & Gas Exploration & Production");
                m.insert(10102030, "Oil & Gas Refining & Marketing");
                m.insert(10102040, "Oil & Gas Storage & Transportation");
                m.insert(10102050, "Coal & Consumable Fuels");

    m.insert(15, "Materials");
        m.insert(1510, "Materials");
            m.insert(151010, "Chemicals");
                m.insert(15101010, "Commodity Chemicals");
                m.insert(15101020, "Diversified Chemicals");
                m.insert(15101030, "Fertilizers & Agricultural Chemicals");
                m.insert(15101040, "Industrial Gases");
                m.insert(15101050, "Specialty Chemicals");
            m.insert(151020, "Construction Materials");
                m.insert(15102010, "Construction Materials");
            m.insert(151030, "Containers & Packaging");
                m.insert(15103010, "Metal & Glass Containers");
                m.insert(15103020, "Paper Packaging");
            m.insert(151040, "Metals & Mining");
                m.insert(15104010, "Aluminum");
                m.insert(15104020, "Diversified Metals & Mining");
                m.insert(15104025, "Copper");
                m.insert(15104030, "Gold");
                m.insert(15104040, "Precious Metals & Minerals");
                m.insert(15104045, "Silver");
                m.insert(15104050, "Steel");
            m.insert(151050, "Paper & Forest Products");
                m.insert(15105010, "Forest Products");
                m.insert(15105020, "Paper Products");


    m.insert(20, "Industrials");
        m.insert(2010, "Capital goods");
            m.insert(201010, "Aerospace & Defense");
                m.insert(20101010, "Aerospace & Defense");
            m.insert(201020, "Building Products");
                m.insert(20102010, "Building Products");
            m.insert(201030, "Construction & Engineering");
                m.insert(20103010, "Construction & Engineering");
            m.insert(201040, "Electrical Equipment");
                m.insert(20104010, "Electrical Components & Equipment");
                m.insert(20104020, "Heavy Electrical Equipment");
            m.insert(201050, "Industrial Conglomerates");
                m.insert(20105010, "Industrial Conglomerates");
            m.insert(201060, "Machinery");
                m.insert(20106010, "Construction Machinery & Heavy Trucks");
                m.insert(20106015, "Agricultural & Farm Machinery");
                m.insert(20106020, "Industrial Machinery");
            m.insert(201070, "Trading Companies & Distributors");
                m.insert(20107010, "Trading Companies & Distributors");
        m.insert(2020, "Commercial & Professional Services");
            m.insert(202010, "Commercial Services & Supplies");
                m.insert(20201010, "Commercial Printing");
                m.insert(20201050, "Environmental & Facilities Services");
                m.insert(20201060, "Office Services & Supplies");
                m.insert(20201070, "Diversified Support Services");
                m.insert(20201080, "Security & Alarm Services");
            m.insert(202020, "Professional Services");
                m.insert(20202010, "Human Resource & Employment Services");
                m.insert(20202020, "Research & Consulting Services");
        m.insert(2030, "Transportation");
            m.insert(203010, "Air Freight & Logistics");
                m.insert(20301010, "Air Freight & Logistics");
            m.insert(203020, "Airlines");
                m.insert(20302010, "Airlines");
            m.insert(203030, "Marine");
                m.insert(20303010, "Marine");
            m.insert(203040, "Road & Rail");
                m.insert(20304010, "Railroads");
                m.insert(20304020, "Trucking");
            m.insert(203050, "Transportation Infrastructure");
                m.insert(20305010, "Airport Services");
                m.insert(20305020, "Highways & Railtracks");
                m.insert(20305030, "Marine Ports & Services");

    m.insert(25, "Consumer Discretionary");
        m.insert(2510, "Automobiles & Components");
            m.insert(251010, "Auto Components    ");
                m.insert(25101010, "Auto Parts & Equipment");
                m.insert(25101020, "Tires & Rubber");
            m.insert(251020, "Automobiles");
                m.insert(25102010, "Automobile Manufacturers");
                m.insert(25102020, "Motorcycle Manufacturers");
        m.insert(2520, "Consumer Durables & Apparel");
            m.insert(252010, "Household Durables");
                m.insert(25201010, "Consumer Electronics");
                m.insert(25201020, "Home Furnishings");
                m.insert(25201030, "Homebuilding");
                m.insert(25201040, "Household Appliances");
                m.insert(25201050, "Housewares & Specialties");
            m.insert(252020, "Leisure Products");
                m.insert(25202010, "Leisure Products");
            m.insert(252030, "Textiles, Apparel & Luxury Goods");
                m.insert(25203010, "Apparel, Accessories & Luxury Goods");
                m.insert(25203020, "Footwear");
                m.insert(25203030, "Textiles");
        m.insert(2530, "Consumer Services");
            m.insert(253010, "Hotels, Restaurants & Leisure");
                m.insert(25301010, "Casinos & Gaming");
                m.insert(25301020, "Hotels, Resorts & Cruise Lines");
                m.insert(25301030, "Leisure Facilities");
                m.insert(25301040, "Restaurants");
            m.insert(253020, "Diversified Consumer Services");
                m.insert(25302010, "Education Services");
                m.insert(25302020, "Specialized Consumer Services");
        m.insert(2550, "Retailing");
            m.insert(255010, "Distributors");
                m.insert(25501010, "Distributors");
            m.insert(255020, "Internet & Direct Marketing Retail");
                m.insert(25502020, "Internet & Direct Marketing Retail");
            m.insert(255030, "Multiline Retail");
                m.insert(25503010, "Department Stores");
                m.insert(25503020, "General Merchandise Stores");
            m.insert(255040, "Specialty Retail");
                m.insert(25504010, "Apparel Retail");
                m.insert(25504020, "Computer & Electronics Retail");
                m.insert(25504030, "Home Improvement Retail");
                m.insert(25504040, "Specialty Stores");
                m.insert(25504050, "Automotive Retail");
                m.insert(25504060, "Homefurnishing Retail");

    m.insert(30, "Consumer Staples");
        m.insert(3010, "Food & Staples Retailing");
            m.insert(301010, "Food & Staples Retailing");
                m.insert(30101010, "Drug Retail");
                m.insert(30101020, "Food Distributors");
                m.insert(30101030, "Food Retail");
                m.insert(30101040, "Hypermarkets & Super Centers");
        m.insert(3020, "Food, Beverage & Tobacco");
            m.insert(302010, "Beverages");
                m.insert(30201010, "Brewers");
                m.insert(30201020, "Distillers & Vintners");
                m.insert(30201030, "Soft Drinks");
            m.insert(302020, "Food Products");
                m.insert(30202010, "Agricultural Products");
                m.insert(30202030, "Packaged Foods & Meats");
            m.insert(302030, "Tobacco");
                m.insert(30203010, "Tobacco");
        m.insert(3030, "Household & Personal Products");
            m.insert(303010, "Household Products");
                m.insert(30301010, "Household Products");
        m.insert(303020, "Personal Products");
                m.insert(30302010, "Personal Products");


    m.insert(35, "Health Care");
        m.insert(3510, "Health Care Equipment & Services");
            m.insert(351010, "Health Care Equipment & Supplies");
                m.insert(35101010, "Health Care Equipment");
                m.insert(35101020, "Health Care Supplies");
            m.insert(351020, "Health Care Providers & Services");
                m.insert(35102010, "Health Care Distributors");
                m.insert(35102015, "Health Care Services");
                m.insert(35102020, "Health Care Facilities");
                m.insert(35102030, "Managed Health Care");
            m.insert(351030, "Health Care Technology");
                m.insert(35103010, "Health Care Technology");
        m.insert(3520, "Pharmaceuticals, Biotechnology & Life");
            m.insert(352010, "Biotechnology");
                m.insert(35201010, "Biotechnology");
            m.insert(352020, "Pharmaceuticals");
                m.insert(35202010, "Pharmaceuticals");
            m.insert(352030, "Life Sciences Tools & Services");
                m.insert(35203010, "Life Sciences Tools & Services");

    m.insert(40, "Financials");
        m.insert(4010, "Banks");
            m.insert(401010, "Banks");
                m.insert(40101010, "Diversified Banks");
                m.insert(40101015, "Regional Banks");
            m.insert(401020, "Thrifts & Mortgage Finance");
                m.insert(40102010, "Thrifts & Mortgage Finance");
        m.insert(4020, "Diversified Financials");
            m.insert(402010, "Diversified Financial Services");
                m.insert(40201020, "Other Diversified Financial Services");
                m.insert(40201030, "Multi-Sector Holdings");
                m.insert(40201040, "Specialized Finance");
            m.insert(402020, "Consumer Finance");
                m.insert(40202010, "Consumer Finance");
            m.insert(402030, "Capital Markets");
                m.insert(40203010, "Asset Management & Custody Banks");
                m.insert(40203020, "Investment Banking & Brokerage");
                m.insert(40203030, "Diversified Capital Markets");
                m.insert(40203040, "Financial Exchanges & Data");
            m.insert(402040, "Mortgage Real Estate Investment Trusts (REITs)");
                m.insert(40204010, "Mortgage REITs");
        m.insert(4030, "Insurance");
        m.insert(403010, "Insurance");
            m.insert(40301010, "Insurance Brokers");
            m.insert(40301020, "Life & Health Insurance");
            m.insert(40301030, "Multi-line Insurance");
            m.insert(40301040, "Property & Casualty Insurance");
            m.insert(40301050, "Reinsurance");

    m.insert(45, "Information Technology");
        m.insert(4510, "Software & Services");
            m.insert(451020, "IT Services");
                m.insert(45102010, "IT Consulting & Other Services");
                m.insert(45102020, "Data Processing & Outsourced Services");
                m.insert(45102030, "Internet Services & Infrastructure");
            m.insert(451030, "Software");
                m.insert(45103010, "Application Software");
                m.insert(45103020, "Systems Software");
        m.insert(4520, "Technology Hardware & Equipment");
            m.insert(452010, "Communications Equipment");
                m.insert(45201020, "Communications Equipment");
            m.insert(452020, "Technology Hardware, Storage & Peripherals");
                m.insert(45202030, "Technology Hardware, Storage & Peripherals");
            m.insert(452030, "Electronic Equipment, Instruments & Components");
                m.insert(45203010, "Electronic Equipment & Instruments");
                m.insert(45203015, "Electronic Components");
                m.insert(45203020, "Electronic Manufacturing Services");
        m.insert(4530, "Semiconductors & Semiconductor");
            m.insert(453010, "Semiconductors & Semiconductor Equipment");
                m.insert(45301010, "Semiconductor Equipment");
                m.insert(45301020, "Semiconductors");



    m.insert(50, "Communication Services");
        m.insert(5010, "Telecommunication Services");
            m.insert(501010, "Diversified Telecommunication Services");
                m.insert(50101010, "Alternative Carriers");
                m.insert(50101020, "Integrated Telecommunication Services");
            m.insert(501020, "Wireless Telecommunication Services");
                m.insert(50102010, "Wireless Telecommunication Services");
        m.insert(5020, "Media & Entertainment");
            m.insert(502010, "Media");
                m.insert(50201010, "Advertising");
                m.insert(50201020, "Broadcasting");
                m.insert(50201030, "Cable & Satellite");
                m.insert(50201040, "Publishing");
            m.insert(502020, "Entertainment");
                m.insert(50202010, "Movies & Entertainment");
                m.insert(50202020, "Interactive Home Entertainment");
            m.insert(502030, "Interactive Media & Services");
                m.insert(50203010, "Interactive Media & Services");

    m.insert(55, "Utilities");
        m.insert(5510, "Utilities");
            m.insert(551010, "Electric Utilities");
                m.insert(55101010, "Electric Utilities");
            m.insert(551020, "Gas Utilities");
                m.insert(55102010, "Gas Utilities");
            m.insert(551030, "Multi-Utilities");
                m.insert(55103010, "Multi-Utilities");
            m.insert(551040, "Water Utilities");
                m.insert(55104010, "Water Utilities");
            m.insert(551050, "Independent Power and Renewable Electricity Producers");
                m.insert(55105010, "Independent Power Producers & Energy Traders");
                m.insert(55105020, "Renewable Electricity");

    m.insert(60, "Real Estate");
        m.insert(6010, "Real Estate");
            m.insert(601010, "Equity Real Estate Investment Trusts (REITs)");
                m.insert(60101010, "Diversified REITs");
                m.insert(60101020, "Industrial REITs");
                m.insert(60101030, "Hotel & Resort REITs");
                m.insert(60101040, "Office REITs");
                m.insert(60101050, "Health Care REITs");
                m.insert(60101060, "Residential REITs");
                m.insert(60101070, "Retail REITs");
                m.insert(60101080, "Specialized REITs");
            m.insert(601020, "Real Estate Management & Development");
                m.insert(60102010, "Diversified Real Estate Activities");
                m.insert(60102020, "Real Estate Operating Companies");
                m.insert(60102030, "Real Estate Development");
                m.insert(60102040, "Real Estate Services");
    m
});

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    pub date: String, // "2021-04-30",
    pub volume: u64,  // 12356

    pub close_price: f64, // 17.68,
    pub high_price: f64,  // 17.68,
    pub low_price: f64,   // 17.68,
    pub open_price: f64,  // 17.68,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub symbol: String,
    pub company_name: String,
    pub exchange: Option<String>,
    pub industry: Option<String>,
    pub website: Option<String>,
    pub investor_website: Option<String>,
    pub description: Option<String>,
    pub primary_sic_code: Option<u64>,
    pub address: Option<String>,
    pub address2: Option<String>,
    pub state: Option<String>,
    pub city: Option<String>,
    pub zip: Option<String>,
    pub country: Option<String>,
    pub phone: Option<String>,
    pub state_of_incorporation: Option<String>,
    pub fiscal_year_end: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CefMeta {
    #[serde(alias = "SponsorId")]
    pub sponsor_id: u32, // 44

    #[serde(alias = "SponsorName")]
    pub sponsor_name: String, // "Franklin Templeton Investments"

    #[serde(alias = "CategoryId")]
    pub category_id: u32,
    #[serde(alias = "CategoryName")]
    pub category: String,
    #[serde(alias = "Strategy")]
    pub strategy: String, // "Fixed Income - Taxable-High Yield"

    #[serde(alias = "Name")]
    pub name: String, // "Aberdeen Japan Equity Fund"
    #[serde(alias = "Ticker")]
    pub symbol: String,

    // distributionRateOnPrice: p.DistributionRatePrice / 100,
    #[serde(alias = "LastUpdated")]
    pub updated_date: String,

    #[serde(alias = "NavTicker")]
    pub nav_symbol: String,
}
