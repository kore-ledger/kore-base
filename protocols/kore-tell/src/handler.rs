// Copyright 2023 Antonio Estévez
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # Handler
//!

use crate::codec::Codec;

pub struct Handler<TCodec>
where
    TCodec: Codec,
{
    pub codec: TCodec,
}
