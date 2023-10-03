// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

//! XCM configuration for Polkadot Bulletin chain.

use crate::{
	bridge_config::ToBridgeHubPolkadotHaulBlobExporter, AllPalletsWithSystem, RuntimeCall,
	RuntimeOrigin,
};

use bridge_runtime_common::messages_xcm_extension::XcmAsPlainPayload;
use codec::{Decode, DecodeLimit, Encode};
use frame_support::{
	ensure, match_types, parameter_types,
	traits::{Contains, Everything, Nothing, ProcessMessageError},
	weights::Weight,
};
use sp_core::ConstU32;
use sp_io::hashing::blake2_256;
use xcm::{latest::prelude::*, VersionedInteriorMultiLocation, VersionedXcm, MAX_XCM_DECODE_DEPTH};
use xcm_builder::{
	CreateMatcher, DispatchBlob, DispatchBlobError, FixedWeightBounds, MatchXcm,
	TrailingSetTopicAsId, UnpaidLocalExporter, WithComputedOrigin,
};
use xcm_executor::{
	traits::{ConvertOrigin, ShouldExecute, WeightTrader, WithOriginFilter},
	Assets, XcmExecutor,
};

// TODO [bridge]: change to actual value here + everywhere where Kawabunga is mentioned
/// Id of the Polkadot parachain that we are going to bridge with.
const KAWABUNGA_PARACHAIN_ID: u32 = 42;

parameter_types! {
	// TODO [bridge]: how we are supposed to set it? Named? ByGenesis - if so, when? After generating
	// chain spec?
	/// The Polkadot Bulletin Chain network ID.
	pub const ThisNetwork: NetworkId = NetworkId::ByGenesis([42u8; 32]);
	/// Our location in the universe of consensus systems.
	pub const UniversalLocation: InteriorMultiLocation = X1(GlobalConsensus(ThisNetwork::get()));

	/// Location of the Kawabunga parachain, relative to this runtime.
	pub KawabungaLocation: MultiLocation = MultiLocation::new(1, X2(
		GlobalConsensus(Polkadot),
		Parachain(KAWABUNGA_PARACHAIN_ID),
	));

	/// The amount of weight an XCM operation takes. This is a safe overestimate.
	pub const BaseXcmWeight: Weight = Weight::from_parts(1_000_000_000, 0);
	/// Maximum number of instructions in a single XCM fragment. A sanity check against weight
	/// calculations getting too crazy.
	pub const MaxInstructions: u32 = 100;
}

match_types! {
	// Only contains Kawabunga parachain location.
	pub type OnlyKawabungaLocation: impl Contains<MultiLocation> = {
		MultiLocation { parents: 1, interior: X2(
			GlobalConsensus(Polkadot),
			Parachain(KAWABUNGA_PARACHAIN_ID),
		) }
	};

	// Supported universal aliases.
	pub type UniversalAliases: impl Contains<(MultiLocation, Junction)> = {
		(
			MultiLocation { parents: 0, interior: Here },
			GlobalConsensus(Polkadot),
		)
	};
}

/// Kawabunga location converter to local root.
pub struct KawabungaParachainAsRoot;

impl ConvertOrigin<RuntimeOrigin> for KawabungaParachainAsRoot {
	fn convert_origin(
		origin: impl Into<MultiLocation>,
		kind: OriginKind,
	) -> Result<RuntimeOrigin, MultiLocation> {
		let origin = origin.into();
		log::trace!(
			target: "xcm::origin_conversion",
			"KawabungaParachainAsRoot origin: {:?}, kind: {:?}",
			origin, kind,
		);
		match (kind, origin) {
			(
				OriginKind::Superuser,
				MultiLocation {
					parents: 1,
					interior: X2(GlobalConsensus(Polkadot), Parachain(KAWABUNGA_PARACHAIN_ID)),
				},
			) => Ok(RuntimeOrigin::root()),
			(_, origin) => Err(origin),
		}
	}
}

/// Weight trader that does nothing.
pub struct NoopTrader;

impl WeightTrader for NoopTrader {
	fn new() -> Self {
		NoopTrader
	}

	fn buy_weight(&mut self, _weight: Weight, _payment: Assets) -> Result<Assets, XcmError> {
		Ok(Assets::new())
	}

	fn refund_weight(&mut self, _weight: Weight) -> Option<MultiAsset> {
		None
	}
}

/// Allows execution from `origin` if it is contained in `AllowedOrigin`
/// and if it is just a straight `Transact` which contains `AllowedCall`.
///
/// That's a 1:1 copy of corresponding Cumulus structure.
pub struct AllowUnpaidTransactsFrom<RuntimeCall, AllowedOrigin>(
	sp_std::marker::PhantomData<(RuntimeCall, AllowedOrigin)>,
);
impl<RuntimeCall: Decode, AllowedOrigin: Contains<MultiLocation>> ShouldExecute
	for AllowUnpaidTransactsFrom<RuntimeCall, AllowedOrigin>
{
	fn should_execute<Call>(
		origin: &MultiLocation,
		instructions: &mut [Instruction<Call>],
		max_weight: Weight,
		_properties: &mut xcm_executor::traits::Properties,
	) -> Result<(), ProcessMessageError> {
		log::trace!(
			target: "xcm::barriers",
			"AllowUnpaidTransactsFrom origin: {:?}, instructions: {:?}, max_weight: {:?}, properties: {:?}",
			origin, instructions, max_weight, _properties,
		);

		// we only allow from configured origins
		ensure!(AllowedOrigin::contains(origin), ProcessMessageError::Unsupported);

		// we expect an XCM program with single `Transact` call
		instructions
			.matcher()
			.assert_remaining_insts(1)?
			.match_next_inst(|inst| match inst {
				Transact { origin_kind: OriginKind::Superuser, .. } => {
					// we allow all calls through the bridge
					Ok(())
				},
				_ => Err(ProcessMessageError::BadFormat),
			})?;

		Ok(())
	}
}

/// The means that we convert an XCM origin `MultiLocation` into the runtime's `Origin` type for
/// local dispatch. This is a conversion function from an `OriginKind` type along with the
/// `MultiLocation` value and returns an `Origin` value or an error.
type LocalOriginConverter = (
	// Currently we only accept XCM messages from Kawabunga and the origin for such messages
	// is local root.
	KawabungaParachainAsRoot,
);

// TODO [bridge]: configure to use bridge blob hauler here
/// Only bridged destination is supported.
pub type XcmRouter = UnpaidLocalExporter<ToBridgeHubPolkadotHaulBlobExporter, UniversalLocation>;

/// The barriers one of which must be passed for an XCM message to be executed.
pub type Barrier = TrailingSetTopicAsId<
	WithComputedOrigin<
		// We only allow unpaid execution from the Kawabunga parachain.
		AllowUnpaidTransactsFrom<RuntimeCall, OnlyKawabungaLocation>,
		UniversalLocation,
		ConstU32<2>,
	>,
>;

/// XCM executor configuration.
pub struct XcmConfig;

impl xcm_executor::Config for XcmConfig {
	type RuntimeCall = RuntimeCall;
	type XcmSender = XcmRouter;
	type AssetTransactor = ();
	type OriginConverter = LocalOriginConverter;
	type IsReserve = ();
	type IsTeleporter = ();
	type UniversalLocation = UniversalLocation;
	type Barrier = Barrier;
	// TODO [bridge]: is it ok to use `FixedWeightBounds` here? We don't have the `pallet-xcm`
	// and IIUC can't use XCM benchmarks because of that?
	type Weigher = FixedWeightBounds<BaseXcmWeight, RuntimeCall, MaxInstructions>;
	type Trader = NoopTrader;
	type ResponseHandler = ();
	type AssetTrap = ();
	type AssetLocker = ();
	type AssetExchanger = ();
	type AssetClaims = ();
	type SubscriptionService = ();
	type PalletInstancesInfo = AllPalletsWithSystem;
	type MaxAssetsIntoHolding = ConstU32<0>;
	type FeeManager = ();
	type MessageExporter = ToBridgeHubPolkadotHaulBlobExporter;
	type UniversalAliases = UniversalAliases;
	type CallDispatcher = WithOriginFilter<Everything>;
	type SafeCallFilter = Everything;
	type Aliasers = Nothing;
}

/// XCM blob dispatcher that executes XCM message at this chain.
///
/// That's a copy of `xcm_builder::BridgeBlobDispatcher` struct. The only difference is
/// that instead of sending XCM further, it dispatches the message immediately.
pub struct ImmediateXcmDispatcher;

impl DispatchBlob for ImmediateXcmDispatcher {
	fn dispatch_blob(blob: XcmAsPlainPayload) -> Result<(), DispatchBlobError> {
		let our_universal = UniversalLocation::get();
		let our_global =
			our_universal.global_consensus().map_err(|()| DispatchBlobError::Unbridgable)?;
		// internally it is the encoded `BridgeMessage`, but it is a private struct, so we
		// are simply decoding pair here
		let (universal_dest, message) = decode_bridge_message(&blob)?;
		let universal_dest: InteriorMultiLocation = universal_dest
			.try_into()
			.map_err(|_| DispatchBlobError::UnsupportedLocationVersion)?;
		// `universal_dest` is the desired destination within the universe: first we need to check
		// we're in the right global consensus.
		let intended_global = universal_dest
			.global_consensus()
			.map_err(|()| DispatchBlobError::NonUniversalDestination)?;
		ensure!(intended_global == our_global, DispatchBlobError::WrongGlobal);
		let message: Xcm<RuntimeCall> =
			message.try_into().map_err(|_| DispatchBlobError::UnsupportedXcmVersion)?;

		log::trace!(
			target: "runtime::xcm",
			"Going to dispatch XCM message from {:?}: {:?}",
			KawabungaLocation::get(),
			message,
		);

		// we allow any calls through XCM, so no limit. It doesn't mean that we really spend
		// the `Weight::MAX` here. Actual message weight is computed by `MessageDispatch`
		// implementation and is limited by the weight of `receive_messsages_proof` call.
		let weight_limit = Weight::MAX;

		// execute the XCM program
		let message_hash = message.using_encoded(blake2_256);
		XcmExecutor::<XcmConfig>::execute_xcm(
			KawabungaLocation::get(),
			message,
			message_hash,
			weight_limit,
		)
		.ensure_complete()
		.map_err(|e| {
			log::trace!(
				target: "runtime::xcm",
				"XCM message from {:?} was dispatched with an error: {:?}",
				KawabungaLocation::get(),
				e,
			);

			// nothing better than this error :/
			DispatchBlobError::RoutingError
		})
	}
}

/// Decode inbound `BridgeMessage` from Kawabunga parachain.
pub(crate) fn decode_bridge_message(
	blob: &XcmAsPlainPayload,
) -> Result<(VersionedInteriorMultiLocation, VersionedXcm<RuntimeCall>), DispatchBlobError> {
	<(VersionedInteriorMultiLocation, VersionedXcm<RuntimeCall>)>::decode_all_with_depth_limit(
		MAX_XCM_DECODE_DEPTH,
		&mut &blob[..],
	)
	.map_err(|_| DispatchBlobError::InvalidEncoding)
}

#[cfg(test)]
pub(crate) mod tests {
	use super::*;
	use crate::{
		bridge_config::{tests::run_test, WithBridgeHubPolkadotMessagesInstance, XCM_LANE},
		BridgePolkadotMessages, Runtime,
	};
	use bp_messages::{
		target_chain::{DispatchMessage, DispatchMessageData, MessageDispatch},
		MessageKey,
	};
	use codec::Encode;
	use pallet_bridge_messages::Config as MessagesConfig;
	use xcm_executor::traits::Properties;

	type Dispatcher =
		<Runtime as MessagesConfig<WithBridgeHubPolkadotMessagesInstance>>::MessageDispatch;

	fn test_storage_key() -> Vec<u8> {
		(*b"test_key").to_vec()
	}

	fn test_storage_value() -> Vec<u8> {
		(*b"test_value").to_vec()
	}

	#[test]
	fn expected_message_from_kawabunga_passes_barrier() {
		// prepare message that we expect to come from the Polkadot BH
		// (everything is relative to Polkadot BH)
		let bridge_hub_universal_location = X2(GlobalConsensus(Polkadot), Parachain(1002));
		let kawabunga_origin = MultiLocation::new(1, Parachain(KAWABUNGA_PARACHAIN_ID));
		let universal_source =
			bridge_hub_universal_location.within_global(kawabunga_origin).unwrap();
		let (local_net, local_sub) = universal_source.split_global().unwrap();
		let mut xcm: Xcm<RuntimeCall> = vec![
			UniversalOrigin(GlobalConsensus(local_net)),
			DescendOrigin(local_sub),
			Transact {
				origin_kind: OriginKind::Superuser,
				require_weight_at_most: Weight::MAX,
				call: RuntimeCall::System(frame_system::Call::remark { remark: vec![42] })
					.encode()
					.into(),
			},
		]
		.into();

		// ensure that it passes local XCM Barrier
		assert_eq!(
			Barrier::should_execute(
				&Here.into(),
				xcm.inner_mut(),
				Weight::MAX,
				&mut Properties { weight_credit: Weight::MAX, message_id: None },
			),
			Ok(())
		);
	}

	pub fn encoded_xcm_message_from_bridge_hub_polkadot_require_wight_at_most() -> Weight {
		Weight::from_parts(20_000_000_000, 8000)
	}

	pub fn encoded_xcm_message_from_bridge_hub_polkadot() -> Vec<u8> {
		let universal_dest: VersionedInteriorMultiLocation =
			X1(GlobalConsensus(crate::xcm_config::ThisNetwork::get())).into();
		let xcm: Xcm<RuntimeCall> = vec![Transact {
			origin_kind: OriginKind::Superuser,
			call: RuntimeCall::System(frame_system::Call::set_storage {
				items: vec![(test_storage_key(), test_storage_value())],
			})
			.encode()
			.into(),
			require_weight_at_most:
				encoded_xcm_message_from_bridge_hub_polkadot_require_wight_at_most(),
		}]
		.into();
		let xcm = VersionedXcm::<RuntimeCall>::V3(xcm);
		// XCM BridgeMessage - a pair of `VersionedInteriorMultiLocation` and `VersionedXcm<()>`
		(universal_dest, xcm).encode()
	}

	#[test]
	fn messages_from_bridge_hub_polkadot_are_dispatched() {
		run_test(|| {
			assert_eq!(frame_support::storage::unhashed::get_raw(&test_storage_key()), None);
			Dispatcher::dispatch(DispatchMessage {
				key: MessageKey { lane_id: XCM_LANE, nonce: 1 },
				data: DispatchMessageData {
					payload: Ok(encoded_xcm_message_from_bridge_hub_polkadot()),
				},
			});
			assert_eq!(
				frame_support::storage::unhashed::get_raw(&test_storage_key()),
				Some(test_storage_value()),
			);
		});
	}

	#[test]
	fn messages_to_bridge_hub_polkadot_are_sent() {
		run_test(|| {
			assert_eq!(
				BridgePolkadotMessages::outbound_lane_data(XCM_LANE).latest_generated_nonce,
				0
			);
			send_xcm::<XcmRouter>(KawabungaLocation::get(), vec![ClearOrigin].into())
				.expect("message is sent");
			assert_ne!(
				BridgePolkadotMessages::outbound_lane_data(XCM_LANE).latest_generated_nonce,
				0
			);
		})
	}

	#[test]
	fn encoded_test_xcm_message_to_bulletin_chain() {
		// this "test" is currently used to encode dummy message for Polkadot BH -> Bulletin
		// bridge. Once we have real sending chain (Kawabunga), it could be removed
		println!("{}", hex::encode(&encoded_xcm_message_from_bridge_hub_polkadot()));
	}
}
