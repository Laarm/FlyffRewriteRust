// The sends of the messenger windows, the friend menu and the block replies (rule C9). The session identifies the sender (VR-001), so the leading own-id fields of the
// legacy packets are gone. `data` of the summon messages is the word the server sent, echoed unchanged (it carries the inventory object id of the scroll in its LOW word).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { BlockReqSchema } from '../../gen/proto/flyff/v1/chat_pb';
import {
  AddFriendCancelReqSchema,
  AddFriendNameRequestReqSchema,
  AddFriendReqSchema,
  AddFriendRequestReqSchema,
  FriendInterceptStateReqSchema,
  GetFriendStateReqSchema,
  RemoveFriendReqSchema,
  SendTagSchema,
  SetFriendStateReqSchema,
  SummonFriendCancelSchema,
  SummonFriendConfirmSchema,
  SummonFriendSchema,
} from '../../gen/proto/flyff/v1/messenger_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

export interface MessengerSenders {
  /** `SendAddFriend(requester ...)`: accept the request of a player. */
  addFriend(requester: number): void;
  /** `SendAddFriendReqest(target)`: ask a player (by character id) to become a friend. */
  addFriendRequest(target: number): void;
  /** `SendAddFriendNameReqest(name)`. */
  addFriendNameRequest(name: string): void;
  /** `SendFriendCancel(requester, ...)`: decline a request. */
  friendCancel(requester: number): void;
  /** `SendGetFriendState()`. */
  getFriendState(): void;
  /** `SendSetState(state)`. */
  setState(state: number): void;
  /** `SendFriendInterceptState(friend)`: toggles the block of a friend. */
  friendInterceptState(friend: number): void;
  /** `SendRemoveFriend(friend)`. */
  removeFriend(friend: number): void;
  /** `SendBlock(gu, name)`: tells `targetName` that a request of his was refused (4 repair, 5 trade busy, 6 friend window open, 7 party window open, 8 guild bank). */
  block(gu: number, targetName: string): void;
  /** `SendMessageNote(friend, text)`: a note for an offline friend. */
  messageNote(friend: number, text: string): void;
  /** `SendSummonFriend(data, name)`. */
  summonFriend(data: number, name: string): void;
  /** `SendSummonFriendConfirm(summoner, data)`. */
  summonFriendConfirm(summonerObj: number, data: number): void;
  /** `SendSummonFriendCancel(summoner, data)`. */
  summonFriendCancel(summonerObj: number, data: number): void;
}

export function messengerSenders(send: Send): MessengerSenders {
  return {
    addFriend(requester) {
      // legacy-send: SendAddFriend
      send({ case: 'addFriendReq', value: create(AddFriendReqSchema, { requester }) });
    },
    addFriendRequest(target) {
      // legacy-send: SendAddFriendReqest
      send({ case: 'addFriendRequestReq', value: create(AddFriendRequestReqSchema, { target }) });
    },
    addFriendNameRequest(name) {
      // legacy-send: SendAddFriendNameReqest
      send({
        case: 'addFriendNameRequestReq',
        value: create(AddFriendNameRequestReqSchema, { name }),
      });
    },
    friendCancel(requester) {
      // legacy-send: SendFriendCancel
      send({ case: 'addFriendCancelReq', value: create(AddFriendCancelReqSchema, { requester }) });
    },
    getFriendState() {
      // legacy-send: SendGetFriendState
      send({ case: 'getFriendStateReq', value: create(GetFriendStateReqSchema, {}) });
    },
    setState(state) {
      // legacy-send: SendSetState
      send({ case: 'setFriendStateReq', value: create(SetFriendStateReqSchema, { state }) });
    },
    friendInterceptState(friend) {
      // legacy-send: SendFriendInterceptState
      send({
        case: 'friendInterceptStateReq',
        value: create(FriendInterceptStateReqSchema, { idFriend: friend }),
      });
    },
    removeFriend(friend) {
      // legacy-send: SendRemoveFriend
      send({ case: 'removeFriendReq', value: create(RemoveFriendReqSchema, { idFriend: friend }) });
    },
    block(gu, targetName) {
      // legacy-send: SendBlock
      send({ case: 'blockReq', value: create(BlockReqSchema, { gu, targetName }) });
    },
    messageNote(friend, text) {
      // legacy-send: SendMessageNote
      send({ case: 'sendTag', value: create(SendTagSchema, { to: friend, text }) });
    },
    summonFriend(data, name) {
      // legacy-send: SendSummonFriend
      send({ case: 'summonFriend', value: create(SummonFriendSchema, { data, name }) });
    },
    summonFriendConfirm(summonerObj, data) {
      // legacy-send: SendSummonFriendConfirm
      send({
        case: 'summonFriendConfirm',
        value: create(SummonFriendConfirmSchema, { summonerObj, data }),
      });
    },
    summonFriendCancel(summonerObj, data) {
      // legacy-send: SendSummonFriendCancel
      send({
        case: 'summonFriendCancel',
        value: create(SummonFriendCancelSchema, { summonerObj, data }),
      });
    },
  };
}
