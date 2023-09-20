//! With Polkadot Bridge Hub bridge configuration.

use crate::{AccountId, Runtime, RuntimeEvent, RuntimeOrigin};

use bp_messages::{
	target_chain::{DispatchMessage, MessageDispatch},
	LaneId, MessageNonce,
};
use bp_parachains::SingleParaStoredHeaderDataBuilder;
use bp_runtime::{messages::MessageDispatchResult, ChainId, UnderlyingChainProvider};
use bridge_runtime_common::{
	messages::{
		source::{
			FromThisChainMaximalOutboundPayloadSize, FromThisChainMessageVerifier,
			TargetHeaderChainAdapter,
		},
		target::SourceHeaderChainAdapter,
		BridgedChainWithMessages, MessageBridge, ThisChainWithMessages,
	},
	messages_xcm_extension::{SenderAndLane, XcmAsPlainPayload},
};
use frame_support::{parameter_types, RuntimeDebug};
use sp_runtime::transaction_validity::{InvalidTransaction, TransactionValidity};
use sp_std::vec::Vec;
use xcm::prelude::*;

/// Lane that we are using to send and receive messages.
pub const XCM_LANE: LaneId = LaneId([0, 0, 0, 0]);

parameter_types! {
	/// A set of message relayers, who are able to submit message delivery transactions
	/// and physically deliver messages on this chain.
	///
	/// It can be changed by the governance later.
	pub storage WhitelistedRelayers: Vec<AccountId> = {
		crate::Sudo::key().map(|sudo_key| sp_std::vec![sudo_key]).unwrap_or_default()
	};

	/// A number of Polkadot mandatory headers that are accepted for free at every
	/// **this chain** block.
	pub const MaxFreePolkadotHeadersPerBlock: u32 = 4;
	/// A number of Polkadot header digests that we keep in the storage.
	pub const PolkadotHeadersToKeep: u32 = 1024;
	/// A name of parachains pallet at Pokadot.
	pub const AtPolkadotParasPalletName: &'static str = bp_polkadot::PARAS_PALLET_NAME;

	/// The Polkadot Chain network ID.
	pub const PolkadotNetwork: NetworkId = Polkadot;
	/// Chain identifier of Polkadot Bridge Hub.
	pub const BridgeHubPolkadotChainId: ChainId = bp_runtime::BRIDGE_HUB_POLKADOT_CHAIN_ID;
	/// A number of Polkadot Bridge Hub head digests that we keep in the storage.
	pub const BridgeHubPolkadotHeadsToKeep: u32 = 1024;
	/// A maximal size of Polkadot Bridge Hub head digest.
	pub const MaxPolkadotBrdgeHubHeadSize: u32 = bp_polkadot::MAX_NESTED_PARACHAIN_HEAD_DATA_SIZE;

	/// All active outbound lanes.
	pub const ActiveOutboundLanes: &'static [LaneId] = &[XCM_LANE];
	/// Maximal number of unrewarded relayer entries.
	pub const MaxUnrewardedRelayerEntriesAtInboundLane: MessageNonce =
		bp_bridge_hub_polkadot::MAX_UNREWARDED_RELAYERS_IN_CONFIRMATION_TX;
	/// Maximal number of unconfirmed messages.
	pub const MaxUnconfirmedMessagesAtInboundLane: MessageNonce =
		bp_bridge_hub_polkadot::MAX_UNCONFIRMED_MESSAGES_IN_CONFIRMATION_TX;

	/// Sending chain location and lane used to communicate with Polkadot Bulletin chain.
	pub FromPolkadotBulletinToBridgeHubPolkadotRoute: SenderAndLane = SenderAndLane::new(
		Here.into(),
		XCM_LANE,
	);

	/// XCM message that is never sent to anyone.
	pub NeverSentMessage: Option<Xcm<()>> = None;
}

/// An instance of `pallet_bridge_grandpa` used to bridge with Polkadot.
pub type WithPolkadotBridgeGrandpaInstance = ();
/// An instance of `pallet_bridge_parachains` used to bridge with Polkadot.
pub type WithPolkadotBridgeParachainsInstance = ();
/// An instance of `pallet_bridge_messages` used to bridge with Polkadot Bridge Hub.
pub type WithBridgeHubPolkadotMessagesInstance = ();

impl pallet_bridge_grandpa::Config<WithPolkadotBridgeGrandpaInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = (); // TODO [bridge]: replace with benchmark results

	type BridgedChain = bp_polkadot::Polkadot;
	type MaxFreeMandatoryHeadersPerBlock = MaxFreePolkadotHeadersPerBlock;
	type HeadersToKeep = PolkadotHeadersToKeep;
}

impl pallet_bridge_parachains::Config<WithPolkadotBridgeParachainsInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = (); // TODO [bridge]: replace with benchmark results

	type BridgesGrandpaPalletInstance = WithPolkadotBridgeGrandpaInstance;
	type ParasPalletName = AtPolkadotParasPalletName;
	type ParaStoredHeaderDataBuilder =
		SingleParaStoredHeaderDataBuilder<bp_bridge_hub_polkadot::BridgeHubPolkadot>;
	type HeadsToKeep = BridgeHubPolkadotHeadsToKeep;
	type MaxParaHeadDataSize = MaxPolkadotBrdgeHubHeadSize;
}

impl pallet_bridge_messages::Config<WithBridgeHubPolkadotMessagesInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = (); // TODO [bridge]: replace with benchmark results

	type BridgedChainId = BridgeHubPolkadotChainId;
	type ActiveOutboundLanes = ActiveOutboundLanes;
	type MaxUnrewardedRelayerEntriesAtInboundLane = MaxUnrewardedRelayerEntriesAtInboundLane;
	type MaxUnconfirmedMessagesAtInboundLane = MaxUnconfirmedMessagesAtInboundLane;

	type MaximalOutboundPayloadSize =
		FromThisChainMaximalOutboundPayloadSize<WithBridgeHubPolkadotMessageBridge>;
	type OutboundPayload = XcmAsPlainPayload;

	type InboundPayload = XcmAsPlainPayload;
	type InboundRelayer = AccountId;
	type DeliveryPayments = ();

	type TargetHeaderChain = TargetHeaderChainAdapter<WithBridgeHubPolkadotMessageBridge>;
	type LaneMessageVerifier = FromThisChainMessageVerifier<WithBridgeHubPolkadotMessageBridge>;
	type DeliveryConfirmationPayments = ();

	type SourceHeaderChain = SourceHeaderChainAdapter<WithBridgeHubPolkadotMessageBridge>;
	type MessageDispatch = FromBridgeHubPolkadotBlobDispatcher;
	type OnMessagesDelivered = ();
}

/// Message bridge with Polkadot Bridge Hub.
pub struct WithBridgeHubPolkadotMessageBridge;

/// Polkadot Bridge Hub headers provider.
pub type BridgeHubPolkadotHeadersProvider = pallet_bridge_parachains::ParachainHeaders<
	Runtime,
	WithPolkadotBridgeParachainsInstance,
	bp_bridge_hub_polkadot::BridgeHubPolkadot,
>;

impl MessageBridge for WithBridgeHubPolkadotMessageBridge {
	const BRIDGED_MESSAGES_PALLET_NAME: &'static str =
		bp_polkadot_bulletin::WITH_POLKADOT_BULLETIN_MESSAGES_PALLET_NAME;
	type ThisChain = PolkadotBulletinChain;
	type BridgedChain = BridgeHubPolkadot;
	type BridgedHeaderChain = BridgeHubPolkadotHeadersProvider;
}

/// BridgeHubPolkadot chain from message lane point of view.
#[derive(RuntimeDebug, Clone, Copy)]
pub struct BridgeHubPolkadot;

impl UnderlyingChainProvider for BridgeHubPolkadot {
	type Chain = bp_bridge_hub_polkadot::BridgeHubPolkadot;
}

impl BridgedChainWithMessages for BridgeHubPolkadot {}

/// BridgeHubRococo chain from message lane point of view.
#[derive(RuntimeDebug, Clone, Copy)]
pub struct PolkadotBulletinChain;

impl UnderlyingChainProvider for PolkadotBulletinChain {
	type Chain = bp_polkadot_bulletin::PolkadotBulletin;
}

impl ThisChainWithMessages for PolkadotBulletinChain {
	type RuntimeOrigin = RuntimeOrigin;
}

// TODO [bridge]: replace with immediate XCM dispatcher
/// Dispatches received XCM messages from the Polkadot Bridge Hub.
pub struct FromBridgeHubPolkadotBlobDispatcher;

impl MessageDispatch for FromBridgeHubPolkadotBlobDispatcher {
	type DispatchPayload = XcmAsPlainPayload;
	type DispatchLevelResult = ();

	fn is_active() -> bool {
		true
	}

	fn dispatch_weight(_message: &mut DispatchMessage<Self::DispatchPayload>) -> Weight {
		Weight::zero()
	}

	fn dispatch(
		_: DispatchMessage<Self::DispatchPayload>,
	) -> MessageDispatchResult<Self::DispatchLevelResult> {
		MessageDispatchResult { unspent_weight: Weight::zero(), dispatch_level_result: () }
	}
}

/// Ensure that the account provided is the whitelisted relayer account.
pub fn ensure_whitelisted_relayer(who: &AccountId) -> TransactionValidity {
	if !WhitelistedRelayers::get().contains(who) {
		return Err(InvalidTransaction::BadSigner.into())
	}

	Ok(Default::default())
}

#[cfg(test)]
pub(crate) mod tests {
	use super::*;
	use crate::{
		BridgePolkadotGrandpa, BridgePolkadotMessages, BridgeRejectObsoleteHeadersAndMessages,
		Executive, RuntimeCall, Signature, SignedExtra, SignedPayload, UncheckedExtrinsic,
		ValidateSigned,
	};
	use bp_header_chain::{justification::GrandpaJustification, HeaderChain, InitializationData};
	use bp_messages::{
		DeliveredMessages, InboundLaneData, OutboundLaneData, UnrewardedRelayer,
		UnrewardedRelayersState,
	};
	use bp_polkadot_core::parachains::{ParaHead, ParaHeadsProof};
	use bp_runtime::{
		record_all_trie_keys, BasicOperatingMode, HeaderIdProvider, Parachain, RawStorageProof,
		StorageProofSize,
	};
	use bridge_runtime_common::{
		assert_complete_bridge_types,
		integrity::{
			assert_complete_bridge_constants, check_message_lane_weights,
			AssertBridgeMessagesPalletConstants, AssertBridgePalletNames, AssertChainConstants,
			AssertCompleteBridgeConstants,
		},
		messages::{
			source::FromBridgedChainMessagesDeliveryProof, target::FromBridgedChainMessagesProof,
		},
		messages_generation::{
			encode_all_messages, encode_lane_data, prepare_messages_storage_proof,
		},
	};
	use codec::Encode;
	use frame_support::assert_ok;
	use sp_api::HeaderT;
	use sp_consensus_grandpa::{AuthorityList, SetId};
	use sp_keyring::AccountKeyring;
	use sp_runtime::{generic::Era, transaction_validity::TransactionValidityError, BuildStorage};
	use sp_trie::{trie_types::TrieDBMutBuilderV1, LayoutV1, MemoryDB, TrieMut};

	const POLKADOT_HEADER_NUMBER: bp_polkadot::BlockNumber = 100;
	const BRIDGE_HUB_HEADER_NUMBER: bp_bridge_hub_polkadot::BlockNumber = 200;

	#[derive(Clone, Copy)]
	enum HeaderType {
		WithMessages,
		WithDeliveredMessages,
	}

	fn relayer_account_at_polkadot() -> bp_polkadot::AccountId {
		[42u8; 32].into()
	}

	fn sudo_signer() -> AccountKeyring {
		AccountKeyring::Alice
	}

	fn relayer_signer() -> AccountKeyring {
		AccountKeyring::Bob
	}

	fn non_relay_signer() -> AccountKeyring {
		AccountKeyring::Charlie
	}

	fn polkadot_initial_header() -> bp_polkadot::Header {
		bp_test_utils::test_header(POLKADOT_HEADER_NUMBER - 1)
	}

	fn polkadot_header(t: HeaderType) -> bp_polkadot::Header {
		let bridge_hub_polkadot_head_storage_proof = bridge_hub_polkadot_head_storage_proof(t);
		let state_root = bridge_hub_polkadot_head_storage_proof.0;
		bp_test_utils::test_header_with_root(POLKADOT_HEADER_NUMBER, state_root)
	}

	fn polkadot_grandpa_justification(t: HeaderType) -> GrandpaJustification<bp_polkadot::Header> {
		bp_test_utils::make_default_justification(&polkadot_header(t))
	}

	fn bridge_hub_polkadot_header(t: HeaderType) -> bp_bridge_hub_polkadot::Header {
		bp_test_utils::test_header_with_root(
			BRIDGE_HUB_HEADER_NUMBER,
			match t {
				HeaderType::WithMessages => bridge_hub_polkadot_message_storage_proof().0,
				HeaderType::WithDeliveredMessages =>
					bridge_hub_polkadot_message_delivery_storage_proof().0,
			},
		)
	}

	fn bridge_hub_polkadot_head_storage_proof(
		t: HeaderType,
	) -> (bp_polkadot::Hash, ParaHeadsProof) {
		let (state_root, proof, _) =
			bp_test_utils::prepare_parachain_heads_proof::<bp_polkadot::Header>(vec![(
				bp_bridge_hub_polkadot::BridgeHubPolkadot::PARACHAIN_ID,
				ParaHead(bridge_hub_polkadot_header(t).encode()),
			)]);
		(state_root, proof)
	}

	fn bridge_hub_polkadot_message_storage_proof() -> (bp_bridge_hub_polkadot::Hash, RawStorageProof)
	{
		prepare_messages_storage_proof::<WithBridgeHubPolkadotMessageBridge>(
			XCM_LANE,
			1..=1,
			None,
			StorageProofSize::Minimal(0),
			vec![42],
			encode_all_messages,
			encode_lane_data,
		)
	}

	fn bridge_hub_polkadot_message_proof(
	) -> FromBridgedChainMessagesProof<bp_bridge_hub_polkadot::Hash> {
		let (_, storage_proof) = bridge_hub_polkadot_message_storage_proof();
		let bridged_header_hash = bridge_hub_polkadot_header(HeaderType::WithMessages).hash();
		FromBridgedChainMessagesProof {
			bridged_header_hash,
			storage_proof,
			lane: XCM_LANE,
			nonces_start: 1,
			nonces_end: 1,
		}
	}

	fn bridge_hub_polkadot_message_delivery_storage_proof(
	) -> (bp_bridge_hub_polkadot::Hash, RawStorageProof) {
		let storage_key = bp_messages::storage_keys::inbound_lane_data_key(
			WithBridgeHubPolkadotMessageBridge::BRIDGED_MESSAGES_PALLET_NAME,
			&XCM_LANE,
		)
		.0;
		let storage_value = InboundLaneData::<AccountId> {
			relayers: vec![UnrewardedRelayer {
				relayer: relayer_signer().into(),
				messages: DeliveredMessages { begin: 1, end: 1 },
			}]
			.into(),
			last_confirmed_nonce: 0,
		}
		.encode();
		let mut root = Default::default();
		let mut mdb = MemoryDB::default();
		{
			let mut trie =
				TrieDBMutBuilderV1::<bp_bridge_hub_polkadot::Hasher>::new(&mut mdb, &mut root)
					.build();
			trie.insert(&storage_key, &storage_value).unwrap();
		}

		let storage_proof =
			record_all_trie_keys::<LayoutV1<bp_bridge_hub_polkadot::Hasher>, _>(&mdb, &root)
				.unwrap();

		(root, storage_proof)
	}

	fn bridge_hub_polkadot_message_delivery_proof(
	) -> FromBridgedChainMessagesDeliveryProof<bp_bridge_hub_polkadot::Hash> {
		let (_, storage_proof) = bridge_hub_polkadot_message_delivery_storage_proof();
		let bridged_header_hash =
			bridge_hub_polkadot_header(HeaderType::WithDeliveredMessages).hash();
		FromBridgedChainMessagesDeliveryProof { bridged_header_hash, storage_proof, lane: XCM_LANE }
	}

	fn polkadot_authority_set() -> AuthorityList {
		bp_test_utils::authority_list()
	}

	fn polkadot_authority_set_id() -> SetId {
		1
	}

	// normally we would simply use `RuntimeCall::dispatch` in tests, but we need to test
	// signed extension here, so we need to generate full-scale transaction and dispatch
	// it using `Executive`
	fn construct_and_apply_extrinsic(
		signer: AccountKeyring,
		call: RuntimeCall,
	) -> sp_runtime::ApplyExtrinsicResult {
		let nonce = frame_system::Account::<Runtime>::get(AccountId::from(signer.clone())).nonce;
		let extra: SignedExtra = (
			frame_system::CheckNonZeroSender::<Runtime>::new(),
			frame_system::CheckSpecVersion::<Runtime>::new(),
			frame_system::CheckTxVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			frame_system::CheckEra::<Runtime>::from(Era::immortal()),
			frame_system::CheckNonce::<Runtime>::from(nonce),
			frame_system::CheckWeight::<Runtime>::new(),
			ValidateSigned,
			BridgeRejectObsoleteHeadersAndMessages,
		);
		let payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
		let signature = payload.using_encoded(|e| signer.sign(e));
		Executive::apply_extrinsic(UncheckedExtrinsic::new_signed(
			call,
			AccountId::from(signer.public()).into(),
			Signature::Sr25519(signature.clone()),
			extra,
		))
	}

	fn assert_ok_ok(apply_result: sp_runtime::ApplyExtrinsicResult) {
		assert_ok!(apply_result);
		assert_ok!(apply_result.unwrap());
	}

	pub fn run_test<T>(test: impl FnOnce() -> T) -> T {
		let mut t = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
		pallet_sudo::GenesisConfig::<Runtime> { key: Some(sudo_signer().into()) }
			.assimilate_storage(&mut t)
			.unwrap();

		sp_io::TestExternalities::new(t).execute_with(|| test())
	}

	fn initialize_polkadot_grandpa_pallet() -> sp_runtime::ApplyExtrinsicResult {
		construct_and_apply_extrinsic(
			sudo_signer(),
			RuntimeCall::Sudo(pallet_sudo::Call::sudo {
				call: Box::new(RuntimeCall::BridgePolkadotGrandpa(
					pallet_bridge_grandpa::Call::initialize {
						init_data: InitializationData {
							header: Box::new(polkadot_initial_header()),
							authority_list: polkadot_authority_set(),
							set_id: polkadot_authority_set_id(),
							operating_mode: BasicOperatingMode::Normal,
						},
					},
				)),
			}),
		)
	}

	fn submit_polkadot_header(
		signer: AccountKeyring,
		t: HeaderType,
	) -> sp_runtime::ApplyExtrinsicResult {
		construct_and_apply_extrinsic(
			signer,
			RuntimeCall::BridgePolkadotGrandpa(
				pallet_bridge_grandpa::Call::submit_finality_proof {
					finality_target: Box::new(polkadot_header(t)),
					justification: polkadot_grandpa_justification(t),
				},
			),
		)
	}

	fn submit_polkadot_bridge_hub_header(
		signer: AccountKeyring,
		t: HeaderType,
	) -> sp_runtime::ApplyExtrinsicResult {
		construct_and_apply_extrinsic(
			signer,
			RuntimeCall::BridgePolkadotParachains(
				pallet_bridge_parachains::Call::submit_parachain_heads {
					at_relay_block: (POLKADOT_HEADER_NUMBER, polkadot_header(t).hash()),
					parachains: vec![(
						bp_bridge_hub_polkadot::BridgeHubPolkadot::PARACHAIN_ID.into(),
						bridge_hub_polkadot_header(t).hash(),
					)],
					parachain_heads_proof: bridge_hub_polkadot_head_storage_proof(t).1,
				},
			),
		)
	}

	fn submit_messages_from_polkadot_bridge_hub(
		signer: AccountKeyring,
	) -> sp_runtime::ApplyExtrinsicResult {
		construct_and_apply_extrinsic(
			signer,
			RuntimeCall::BridgePolkadotMessages(
				pallet_bridge_messages::Call::receive_messages_proof {
					relayer_id_at_bridged_chain: relayer_account_at_polkadot(),
					proof: bridge_hub_polkadot_message_proof(),
					messages_count: 1,
					dispatch_weight: Weight::zero(),
				},
			),
		)
	}

	fn submit_confirmations_from_polkadot_bridge_hub(
		signer: AccountKeyring,
	) -> sp_runtime::ApplyExtrinsicResult {
		construct_and_apply_extrinsic(
			signer,
			RuntimeCall::BridgePolkadotMessages(
				pallet_bridge_messages::Call::receive_messages_delivery_proof {
					proof: bridge_hub_polkadot_message_delivery_proof(),
					relayers_state: UnrewardedRelayersState {
						unrewarded_relayer_entries: 1,
						messages_in_oldest_entry: 1,
						total_messages: 1,
						last_delivered_nonce: 1,
					},
				},
			),
		)
	}

	fn emulate_sent_messages() {
		pallet_bridge_messages::OutboundLanes::<Runtime, WithBridgeHubPolkadotMessagesInstance>::insert(
			XCM_LANE,
			OutboundLaneData {
				oldest_unpruned_nonce: 1,
				latest_received_nonce: 0,
				latest_generated_nonce: 1,
			},
		);
	}

	#[test]
	fn sudo_account_is_in_whitelisted_relayers_by_default() {
		run_test(|| {
			// until it is explicitly changed, sudo may submit bridge transactions
			assert_eq!(WhitelistedRelayers::get(), vec![sudo_signer().into()]);
		})
	}

	#[test]
	fn may_change_whitelisted_relayers_set_using_sudo() {
		run_test(|| {
			let whitelisted_relayers_key = WhitelistedRelayers::key().to_vec();
			let new_whitelisted_relayers = vec![AccountId::from(relayer_signer())].encode();

			// sudo may change the whitelisted relayers set
			assert_ok_ok(construct_and_apply_extrinsic(
				sudo_signer(),
				RuntimeCall::Sudo(pallet_sudo::Call::sudo {
					call: Box::new(RuntimeCall::System(frame_system::Call::set_storage {
						items: vec![(whitelisted_relayers_key, new_whitelisted_relayers)],
					})),
				}),
			));

			// and then it itself is missing from the set
			assert_eq!(WhitelistedRelayers::get(), vec![relayer_signer().into()]);
		});
	}

	#[test]
	fn may_initialize_grandpa_pallet_using_sudo() {
		run_test(|| {
			assert_eq!(BridgePolkadotGrandpa::best_finalized(), None);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());
			assert_eq!(
				BridgePolkadotGrandpa::best_finalized(),
				Some(polkadot_initial_header().id())
			);
		});
	}

	#[test]
	fn whitelisted_relayer_may_submit_polkadot_headers() {
		run_test(|| {
			WhitelistedRelayers::set(&vec![relayer_signer().into()]);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());

			// whitelisted relayer may submit regular Polkadot headers delivery transactions
			assert_eq!(
				BridgePolkadotGrandpa::best_finalized(),
				Some(polkadot_initial_header().id())
			);
			assert_ok_ok(submit_polkadot_header(relayer_signer(), HeaderType::WithMessages));
			assert_eq!(
				BridgePolkadotGrandpa::best_finalized(),
				Some(polkadot_header(HeaderType::WithMessages).id())
			);
		})
	}

	#[test]
	fn non_relayer_may_not_submit_polkadot_headers() {
		run_test(|| {
			run_test(|| {
				assert_ok_ok(initialize_polkadot_grandpa_pallet());

				// non-whitelisted account may submit regular Polkadot headers delivery transactions
				assert_eq!(
					BridgePolkadotGrandpa::best_finalized(),
					Some(polkadot_initial_header().id())
				);
				// can't use assert_noop here, because we need to mutate storage inside
				// the `construct_and_apply_extrinsic`
				assert_eq!(
					submit_polkadot_header(non_relay_signer(), HeaderType::WithMessages),
					Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner))
				);
				assert_eq!(
					BridgePolkadotGrandpa::best_finalized(),
					Some(polkadot_initial_header().id())
				);
			})
		})
	}

	#[test]
	fn whitelisted_relayer_may_submit_polkadot_bridge_hub_headers() {
		run_test(|| {
			WhitelistedRelayers::set(&vec![relayer_signer().into()]);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());
			assert_ok_ok(submit_polkadot_header(relayer_signer(), HeaderType::WithMessages));

			// whitelisted relayer may submit regular Polkadot BH headers delivery transactions
			assert_eq!(
				BridgeHubPolkadotHeadersProvider::finalized_header_state_root(
					bridge_hub_polkadot_header(HeaderType::WithMessages).hash()
				),
				None,
			);
			assert_ok_ok(submit_polkadot_bridge_hub_header(
				relayer_signer(),
				HeaderType::WithMessages,
			));
			assert_eq!(
				BridgeHubPolkadotHeadersProvider::finalized_header_state_root(
					bridge_hub_polkadot_header(HeaderType::WithMessages).hash()
				),
				Some(*bridge_hub_polkadot_header(HeaderType::WithMessages).state_root())
			);
		})
	}

	#[test]
	fn non_relayer_may_not_submit_polkadot_bridge_hub_headers() {
		run_test(|| {
			assert_ok_ok(initialize_polkadot_grandpa_pallet());

			// whitelisted relayer may NOT submit regular Polkadot BH headers delivery transactions
			assert_eq!(
				BridgeHubPolkadotHeadersProvider::finalized_header_state_root(
					bridge_hub_polkadot_header(HeaderType::WithMessages).hash()
				),
				None,
			);
			// can't use assert_noop here, because we need to mutate storage inside
			// the `construct_and_apply_extrinsic`
			assert_eq!(
				submit_polkadot_bridge_hub_header(relayer_signer(), HeaderType::WithMessages),
				Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner)),
			);
			assert_eq!(
				BridgeHubPolkadotHeadersProvider::finalized_header_state_root(
					bridge_hub_polkadot_header(HeaderType::WithMessages).hash()
				),
				None
			);
		})
	}

	#[test]
	fn whitelisted_relayer_may_deliver_messages_from_polkadot_bridge_hub() {
		run_test(|| {
			WhitelistedRelayers::set(&vec![relayer_signer().into()]);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());
			assert_ok_ok(submit_polkadot_header(relayer_signer(), HeaderType::WithMessages));
			assert_ok_ok(submit_polkadot_bridge_hub_header(
				relayer_signer(),
				HeaderType::WithMessages,
			));

			// whitelisted relayer may deliver messages from Polkadot BH
			assert!(BridgePolkadotMessages::inbound_lane_data(XCM_LANE).relayers.is_empty());
			assert_ok_ok(submit_messages_from_polkadot_bridge_hub(relayer_signer()));
			assert!(!BridgePolkadotMessages::inbound_lane_data(XCM_LANE).relayers.is_empty());
		})
	}

	#[test]
	fn non_relayer_may_not_deliver_messages_from_polkadot_bridge_hub() {
		run_test(|| {
			WhitelistedRelayers::set(&vec![relayer_signer().into()]);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());
			assert_ok_ok(submit_polkadot_header(relayer_signer(), HeaderType::WithMessages));
			assert_ok_ok(submit_polkadot_bridge_hub_header(
				relayer_signer(),
				HeaderType::WithMessages,
			));

			// non relayer may NOT deliver messages from Polkadot BH
			assert!(BridgePolkadotMessages::inbound_lane_data(XCM_LANE).relayers.is_empty());
			assert_eq!(
				submit_messages_from_polkadot_bridge_hub(non_relay_signer()),
				Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner)),
			);
			assert!(BridgePolkadotMessages::inbound_lane_data(XCM_LANE).relayers.is_empty());
		})
	}

	#[test]
	fn whitelisted_relayer_may_deliver_confirmations_from_polkadot_bridge_hub() {
		run_test(|| {
			WhitelistedRelayers::set(&vec![relayer_signer().into()]);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());
			assert_ok_ok(submit_polkadot_header(
				relayer_signer(),
				HeaderType::WithDeliveredMessages,
			));
			assert_ok_ok(submit_polkadot_bridge_hub_header(
				relayer_signer(),
				HeaderType::WithDeliveredMessages,
			));
			emulate_sent_messages();

			// whitelisted relayer may deliver messages from Polkadot BH
			assert_eq!(
				BridgePolkadotMessages::outbound_lane_data(XCM_LANE).latest_received_nonce,
				0
			);
			assert_ok_ok(submit_confirmations_from_polkadot_bridge_hub(relayer_signer()));
			assert_ne!(
				BridgePolkadotMessages::outbound_lane_data(XCM_LANE).latest_received_nonce,
				0
			);
		})
	}

	#[test]
	fn non_relayer_may_not_deliver_confirmations_from_polkadot_bridge_hub() {
		run_test(|| {
			WhitelistedRelayers::set(&vec![relayer_signer().into()]);
			assert_ok_ok(initialize_polkadot_grandpa_pallet());
			assert_ok_ok(submit_polkadot_header(
				relayer_signer(),
				HeaderType::WithDeliveredMessages,
			));
			assert_ok_ok(submit_polkadot_bridge_hub_header(
				relayer_signer(),
				HeaderType::WithDeliveredMessages,
			));
			emulate_sent_messages();

			// non relayer may NOT deliver messages from Polkadot BH
			assert_eq!(
				BridgePolkadotMessages::outbound_lane_data(XCM_LANE).latest_received_nonce,
				0
			);
			assert_eq!(
				submit_confirmations_from_polkadot_bridge_hub(non_relay_signer()),
				Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner)),
			);
			assert_eq!(
				BridgePolkadotMessages::outbound_lane_data(XCM_LANE).latest_received_nonce,
				0
			);
		})
	}

	#[test]
	fn ensure_lane_weights_are_correct() {
		check_message_lane_weights::<
			bp_polkadot_bulletin::PolkadotBulletin,
			Runtime,
			WithBridgeHubPolkadotMessagesInstance,
		>(
			bp_bridge_hub_polkadot::EXTRA_STORAGE_PROOF_SIZE,
			bp_polkadot_bulletin::MAX_UNREWARDED_RELAYERS_IN_CONFIRMATION_TX,
			bp_polkadot_bulletin::MAX_UNCONFIRMED_MESSAGES_IN_CONFIRMATION_TX,
			false,
		);
	}

	#[test]
	fn ensure_bridge_integrity() {
		assert_complete_bridge_types!(
			runtime: Runtime,
			with_bridged_chain_grandpa_instance: WithPolkadotBridgeGrandpaInstance,
			with_bridged_chain_messages_instance: WithBridgeHubPolkadotMessagesInstance,
			bridge: WithBridgeHubPolkadotMessageBridge,
			this_chain: bp_polkadot_bulletin::PolkadotBulletin,
			bridged_chain: bp_polkadot::Polkadot,
		);

		assert_complete_bridge_constants::<
			Runtime,
			WithPolkadotBridgeGrandpaInstance,
			WithBridgeHubPolkadotMessagesInstance,
			WithBridgeHubPolkadotMessageBridge,
		>(AssertCompleteBridgeConstants {
			this_chain_constants: AssertChainConstants {
				block_length: bp_polkadot_bulletin::BlockLength::get(),
				block_weights: bp_polkadot_bulletin::BlockWeights::get(),
			},
			messages_pallet_constants: AssertBridgeMessagesPalletConstants {
				max_unrewarded_relayers_in_bridged_confirmation_tx:
					bp_bridge_hub_polkadot::MAX_UNREWARDED_RELAYERS_IN_CONFIRMATION_TX,
				max_unconfirmed_messages_in_bridged_confirmation_tx:
					bp_bridge_hub_polkadot::MAX_UNCONFIRMED_MESSAGES_IN_CONFIRMATION_TX,
				bridged_chain_id: bp_runtime::BRIDGE_HUB_POLKADOT_CHAIN_ID,
			},
			pallet_names: AssertBridgePalletNames {
				with_this_chain_messages_pallet_name:
					bp_polkadot_bulletin::WITH_POLKADOT_BULLETIN_MESSAGES_PALLET_NAME,
				with_bridged_chain_grandpa_pallet_name:
					bp_polkadot::WITH_POLKADOT_GRANDPA_PALLET_NAME,
				with_bridged_chain_messages_pallet_name:
					bp_bridge_hub_polkadot::WITH_BRIDGE_HUB_POLKADOT_MESSAGES_PALLET_NAME,
			},
		});
	}
}
