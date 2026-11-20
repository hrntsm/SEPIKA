// 先端荷重の合計50 kN、柱面から5.7 m、柱内法3.5 m、階高4 m。
pub const DEMAND_KN_M: f64 = 249.375;

// RC/SRC梁の主筋群間隔378 mm、柱の主筋群間隔478 mm、ヤング係数比8.2。
pub const RC_LONG_ALLOWABLE_KN_M: f64 = 258.434807904;

// 有効幅450 mm、ウェブ体積888552 mm³、2-D13@425、帯筋対象幅600 mm。
pub const SRC_SHORT_ALLOWABLE_KN_M: f64 = 322.455731907881;

// 総体積139671600 mm³、ウェブ体積1507752 mm³。L字形で式(解20.12)を採用。
pub const STEEL_SHORT_ALLOWABLE_KN_M: f64 = 510.4485444217312;
